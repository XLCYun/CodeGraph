package main

import (
	"go/version"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
)

// installedDirectories 只枚举常规安装线索的直接子目录，不执行 go 或下载工具链。
func installedDirectories(root string) []string {
	entries, _ := os.ReadDir(root)
	paths := []string{}
	for _, entry := range entries {
		full := filepath.Join(root, entry.Name())
		if info, err := os.Stat(full); err == nil && info.IsDir() {
			paths = append(paths, full)
		}
	}
	return paths
}

// sdkRoots 将显式环境、标准安装与 PATH 可执行文件作为静态发现线索。
func sdkRoots() []string {
	roots := []string{}
	for _, key := range []string{"CODEGRAPH_GO_TARGET_SDK", "CODEGRAPH_GO_SDK", "GOROOT"} {
		if root := os.Getenv(key); root != "" {
			roots = append(roots, root)
		}
	}
	roots = append(roots, "/usr/local/go", "/usr/lib/go", "/opt/go")
	for _, key := range []string{"ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"} {
		if base := os.Getenv(key); base != "" {
			roots = append(roots, filepath.Join(base, "Go"), filepath.Join(base, "Programs", "Go"))
		}
	}
	home, _ := os.UserHomeDir()
	for _, root := range installedDirectories(filepath.Join(home, "sdk")) {
		if strings.HasPrefix(filepath.Base(root), "go") {
			roots = append(roots, root)
		}
	}
	for _, prefix := range []string{"/opt/homebrew/opt", "/usr/local/opt"} {
		for _, root := range installedDirectories(prefix) {
			name := filepath.Base(root)
			if name == "go" || strings.HasPrefix(name, "go@") {
				roots = append(roots, filepath.Join(root, "libexec"))
			}
		}
	}
	executable := "go"
	if runtime.GOOS == "windows" {
		executable += ".exe"
	}
	for _, directory := range filepath.SplitList(os.Getenv("PATH")) {
		if full, err := filepath.EvalSymlinks(filepath.Join(directory, executable)); err == nil {
			roots = append(roots, filepath.Dir(filepath.Dir(full)))
		}
	}
	return roots
}

// sdkCatalog 每个 major/minor 只保留最高稳定补丁，项目层继续执行最低版本与精确 minor 政策。
func sdkCatalog(roots []string) ([]platform, *analysisError) {
	best := map[string]platform{}
	for _, root := range roots {
		if !filepath.IsAbs(root) {
			continue
		}
		resolved, err := filepath.EvalSymlinks(root)
		if err != nil {
			continue
		}
		contents, err := os.ReadFile(filepath.Join(resolved, "VERSION"))
		if err != nil {
			continue
		}
		text := strings.TrimSpace(strings.SplitN(string(contents), "\n", 2)[0])
		if !strings.HasPrefix(text, "go") {
			continue
		}
		current := strings.TrimPrefix(text, "go")
		if !validSDKVersion(current) {
			continue
		}
		valid := true
		for _, source := range []string{"src/builtin/builtin.go", "src/runtime/runtime.go"} {
			if info, err := os.Stat(filepath.Join(resolved, filepath.FromSlash(source))); err != nil || !info.Mode().IsRegular() {
				valid = false
			}
		}
		if !valid {
			continue
		}
		metadata := platformMetadata{GOOS: runtime.GOOS, GOARCH: runtime.GOARCH}
		if runtime.GOARCH == "amd64" {
			metadata.GOAMD64 = "v1"
		}
		candidate := platform{ID: "go-sdk-" + current, Kind: "go-sdk", Version: current, Root: resolved, Metadata: metadata}
		minor := version.Lang(text)
		old, ok := best[minor]
		if !ok || version.Compare(text, "go"+old.Version) > 0 {
			best[minor] = candidate
		}
	}
	if len(best) == 0 {
		return nil, fail("target_unavailable", "未找到本机 Go SDK 标准库源码，请安装项目目标对应的 Go SDK")
	}
	result := []platform{}
	for _, candidate := range best {
		result = append(result, candidate)
	}
	sort.Slice(result, func(i, j int) bool { return version.Compare("go"+result[i].Version, "go"+result[j].Version) < 0 })
	return result, nil
}

// discoverInstalledSDKs 冻结本次分析使用的安装目录，不调用项目工具。
func discoverInstalledSDKs() ([]platform, *analysisError) { return sdkCatalog(sdkRoots()) }
