package main

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const maxSelectionFiles = 100000

// checkpoint 让配置加载、源码枚举和 import 闭包共享请求的取消与期限。
func (p *project) checkpoint() *analysisError {
	if p.Context == nil || p.Context.Err() == nil {
		return nil
	}
	if p.Context.Err() == context.Canceled {
		return fail("cancelled", "Go discovery cancelled")
	}
	return fail("resource_limit", "Go discovery exceeded analysis_timeout_ms")
}

// discoverProjectFiles 只枚举根配置准入的模块包，replacement 与 SDK 始终仅作上下文。
func discoverProjectFiles(p *project, includeTests bool, catalog []platform) ([]string, *analysisError) {
	sdks, problem := selectSDKVersions(p, catalog)
	if problem != nil {
		return nil, problem
	}
	files := map[string]bool{}
	for _, module := range p.Modules {
		var failure *analysisError
		err := filepath.WalkDir(module.Dir, func(filename string, entry fs.DirEntry, walkErr error) error {
			if problem := p.checkpoint(); problem != nil {
				failure = problem
				return fs.SkipAll
			}
			if walkErr != nil {
				return walkErr
			}
			name := entry.Name()
			if entry.IsDir() && filename != module.Dir {
				if name == "vendor" || name == "testdata" || strings.HasPrefix(name, ".") || strings.HasPrefix(name, "_") {
					return fs.SkipDir
				}
				if _, err := os.Lstat(filepath.Join(filename, "go.mod")); err == nil {
					return fs.SkipDir
				} else if !os.IsNotExist(err) {
					return err
				}
			}
			if entry.Type()&fs.ModeSymlink != 0 {
				if err := within(p.Root, filename); err != nil {
					return err
				}
				if info, err := os.Stat(filename); err != nil {
					return err
				} else if info.IsDir() {
					failure = fail("unsupported_configuration", "Go package directory symlink is unsupported")
					return fs.SkipAll
				}
			}
			if entry.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasPrefix(name, ".") || strings.HasPrefix(name, "_") {
				return nil
			}
			rel, err := filepath.Rel(p.Root, filename)
			if err != nil {
				return err
			}
			rel = filepath.ToSlash(rel)
			if _, err := relativeFile(p.Root, rel); err != nil {
				return err
			}
			if _, err := safeFile(p.Root, filename); err != nil {
				return err
			}
			if !includeTests && strings.HasSuffix(name, "_test.go") {
				// 关闭测试时只保留确定的角色，不让测试内容改变生产配置。
			} else {
				if _, problem := hasTargetConstraint(p.Root, rel); problem != nil {
					failure = problem
					return fs.SkipAll
				}
				active, problem := matchesSupportedTarget(p.Root, rel, sdks[0])
				if problem != nil {
					failure = problem
					return fs.SkipAll
				}
				if !active {
					return nil
				}
			}
			files[rel] = true
			if len(files) > maxSelectionFiles {
				failure = fail("resource_limit", "Go discovery exceeds file limit")
				return fs.SkipAll
			}
			return nil
		})
		if failure != nil {
			return nil, failure
		}
		if err != nil {
			return nil, fail("unsupported_configuration", err.Error())
		}
	}
	result := make([]string, 0, len(files))
	for file := range files {
		result = append(result, file)
	}
	sort.Strings(result)
	return result, nil
}

// goSourceName 与 go/build 一致忽略以下划线或点开头的包内文件。
func goSourceName(name string) bool {
	return strings.HasSuffix(name, ".go") && !strings.HasPrefix(name, ".") && !strings.HasPrefix(name, "_")
}
