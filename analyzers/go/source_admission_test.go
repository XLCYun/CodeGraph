package main

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestSDKImportsLocalContext 验证标准库导入仍优先使用仓内模块及替换，且不扩大图选集。
func TestSDKImportsLocalContext(t *testing.T) {
	old, _ := fixtureCrossMinorSDKs(t)
	for _, mapping := range []string{"replacement", "workspace", "replacement_targets"} {
		t.Run(mapping, func(t *testing.T) {
			root := t.TempDir()
			mod := "module example.test/app\ngo " + old.Version + "\n"
			if strings.HasPrefix(mapping, "replacement") {
				mod += "replace cmp => ../dep\n"
			} else {
				writeFixture(t, root, "go.work", "go "+old.Version+"\nuse (\n ./app\n ./dep\n)\n")
			}
			writeFixture(t, root, "app/go.mod", mod)
			writeFixture(t, root, "app/a.go", "package app\nimport \"fmt\"\nfunc Run(){fmt.Println(1)}\n")
			writeFixture(t, root, "dep/go.mod", "module cmp\ngo "+old.Version+"\n")
			writeFixture(t, root, "dep/cmp.go", "package cmp\nfunc Local(){}\n")
			writeFixture(t, root, "dep/optional_windows.go", "package cmp\nfunc WindowsOnly(){}\n")
			before := snapshotFixture(t, root)
			sdks := []platform{old}
			if mapping == "replacement_targets" {
				writeFixture(t, root, "app/target_linux.go", "package app\nfunc Linux(){}\n")
				writeFixture(t, root, "app/target_windows.go", "package app\nfunc Windows(){}\n")
				linux, windows := old, old
				linux.ID, windows.ID = "linux", "windows"
				linux.Metadata = platformMetadata{GOOS: "linux", GOARCH: "amd64", GOAMD64: "v1"}
				windows.Metadata = platformMetadata{GOOS: "windows", GOARCH: "amd64", GOAMD64: "v1"}
				sdks = []platform{linux, windows}
				before = snapshotFixture(t, root)
			}
			req := fixtureVersionRequest(t, root, sdks, "app/a.go")
			result := analyze(context.Background(), req)
			if result.Outcome != "ok" || result.Graph == nil {
				t.Fatalf("SDK-to-local import rejected: %+v", result.Error)
			}
			if findNode(result.Graph, "go:app/a.go#func:Run") == nil || findNode(result.Graph, "go:dep/cmp.go#func:Local") != nil || len(result.Evidence.Files) != 1 || result.Evidence.Files[0].Path != "app/a.go" {
				t.Fatalf("SDK context changed graph selection: %+v", result)
			}
			if len(result.Evidence.Configurations) != len(sdks) {
				t.Fatalf("SDK context changed target selection: %+v", result.Evidence)
			}
			for index, config := range result.Evidence.Configurations {
				if config.PlatformIDs[0] != sdks[index].ID {
					t.Fatalf("SDK context changed target identity: %+v", config)
				}
				// 保留旧来源时序，首轮的 SDK 间接替换只在后续配置证据中出现。
				wantSources := []string{"app/go.mod"}
				if mapping == "workspace" {
					wantSources = []string{"app/go.mod", "dep/go.mod", "go.work"}
				} else if index > 0 {
					wantSources = []string{"app/go.mod", "dep/go.mod"}
				}
				if strings.Join(config.SourcePaths, "|") != strings.Join(wantSources, "|") {
					t.Fatalf("SDK context changed configuration evidence: %v, want %v", config.SourcePaths, wantSources)
				}
			}
			after := snapshotFixture(t, root)
			if len(before) != len(after) {
				t.Fatal("analysis changed repository paths")
			}
			for name, content := range before {
				if after[name] != content {
					t.Fatalf("analysis changed %s", name)
				}
			}
		})
	}
}

// TestSDKImportsLocalContextValidation 保留标准库间接引入仓内源码时的标签和路径准入检查。
func TestSDKImportsLocalContextValidation(t *testing.T) {
	old, _ := fixtureCrossMinorSDKs(t)
	for _, invalid := range []string{"build_tag", "outside_symlink"} {
		t.Run(invalid, func(t *testing.T) {
			root := t.TempDir()
			writeFixture(t, root, "go.mod", "module example.test/app\ngo "+old.Version+"\nreplace cmp => ./dep\n")
			writeFixture(t, root, "a.go", "package app\nimport \"fmt\"\nfunc Run(){fmt.Println(1)}\n")
			writeFixture(t, root, "dep/go.mod", "module cmp\ngo "+old.Version+"\n")
			writeFixture(t, root, "dep/cmp.go", "package cmp\nfunc Local(){}\n")
			if invalid == "build_tag" {
				writeFixture(t, root, "dep/optional.go", "//go:build feature\n\npackage cmp\n")
			} else {
				outside := t.TempDir()
				writeFixture(t, outside, "optional.go", "package cmp\n")
				if err := os.Symlink(filepath.Join(outside, "optional.go"), filepath.Join(root, "dep", "optional.go")); err != nil {
					t.Skipf("symlink unavailable: %v", err)
				}
			}
			result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{old}, "a.go"))
			if result.Error == nil || result.Graph != nil {
				t.Fatalf("invalid SDK-to-local context admitted: %+v", result)
			}
			if invalid == "build_tag" && (result.Error.Code != "unsupported_configuration" || !strings.Contains(result.Error.Message, "unsupported build tag: feature")) {
				t.Fatalf("build tag lost its configuration error: %+v", result.Error)
			}
			if invalid == "outside_symlink" && !strings.Contains(result.Error.Message, "path escapes allowed root") {
				t.Fatalf("source path boundary was not checked: %+v", result.Error)
			}
		})
	}
}
