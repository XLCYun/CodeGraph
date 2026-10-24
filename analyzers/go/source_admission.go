package main

import (
	"fmt"
	"go/build"
	"go/parser"
	"go/token"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// sourceAdmission 持有准备阶段登记的包语言和替换模块，提取阶段只查询该索引。
type sourceAdmission struct {
	Project        *project
	Packages       map[string]*module
	ContextModules map[string]*module
}

// newSourceAdmission 为一个独立项目建立源码准入域，不共享其他 module 的替换配置。
func newSourceAdmission(p *project) *sourceAdmission {
	return &sourceAdmission{Project: p, Packages: map[string]*module{}, ContextModules: map[string]*module{}}
}

// moduleForPackage 只查询准备阶段已登记的包，避免提取时改变配置来源。
func (s *sourceAdmission) moduleForPackage(dir string) (*module, error) {
	if m := s.Packages[dir]; m != nil {
		return m, nil
	}
	return nil, fmt.Errorf("package is outside admitted module: %s", dir)
}

// registerPackage 在准备阶段登记包的语言声明，本地 replace 的配置在证据冻结前读取。
func (s *sourceAdmission) registerPackage(dir string) (*module, error) {
	p := s.Project
	if m := p.moduleForFile(dir); m != nil {
		s.Packages[dir] = m
		return m, nil
	}
	base := ""
	for _, replacement := range p.Replace {
		if within(replacement, dir) == nil && !crossedModuleBoundary(replacement, dir) && len(replacement) > len(base) {
			base = replacement
		}
	}
	if base == "" {
		return nil, fmt.Errorf("package is outside admitted module: %s", dir)
	}
	if m := s.ContextModules[base]; m != nil {
		s.Packages[dir] = m
		return m, nil
	}
	m, err := parseModContext(p.Context, p.Root, filepath.Join(base, "go.mod"))
	if err != nil {
		return nil, fmt.Errorf("local replacement go.mod: %w", err)
	}
	s.ContextModules[base] = m
	s.Packages[dir] = m
	return m, nil
}

// contextFiles 登记整个仓内导入闭包，合并未选源码的目标条件并检查准入边界。
func (s *sourceAdmission) contextFiles(selected []string, includeTests bool, sdk platform) ([]string, *analysisError) {
	p := s.Project
	dirs := map[string]bool{}
	for _, rel := range selected {
		if !includeTests && strings.HasSuffix(rel, "_test.go") {
			continue
		}
		filename := filepath.Join(p.Root, filepath.FromSlash(rel))
		if p.moduleForFile(filename) == nil {
			return nil, fail("unsupported_configuration", "selected file is outside admitted module", rel)
		}
		dirs[filepath.Dir(filename)] = true
	}
	files := []string{}
	queue := []string{}
	for dir := range dirs {
		queue = append(queue, dir)
	}
	sort.Strings(queue)
	visited := map[string]bool{}
	for len(queue) > 0 {
		if problem := p.checkpoint(); problem != nil {
			return nil, problem
		}
		dir := queue[0]
		queue = queue[1:]
		if visited[dir] {
			continue
		}
		visited[dir] = true
		m, err := s.registerPackage(dir)
		if problem := p.checkpoint(); problem != nil {
			return nil, problem
		}
		if err != nil {
			return nil, fail("unsupported_configuration", err.Error())
		}
		if problem := m.validateSDKVersion(sdk); problem != nil {
			return nil, problem
		}
		entries, err := os.ReadDir(dir)
		if err != nil {
			return nil, fail("object_extraction_failed", err.Error())
		}
		for _, entry := range entries {
			if problem := p.checkpoint(); problem != nil {
				return nil, problem
			}
			if entry.IsDir() || !goSourceName(entry.Name()) || ((!includeTests || !dirs[dir]) && strings.HasSuffix(entry.Name(), "_test.go")) {
				continue
			}
			filename := filepath.Join(dir, entry.Name())
			data, err := safeFile(p.Root, filename)
			if err != nil {
				return nil, fail("unsupported_configuration", err.Error())
			}
			rel, _ := filepath.Rel(p.Root, filename)
			files = append(files, filepath.ToSlash(rel))
			if _, problem := hasTargetConstraint(p.Root, filepath.ToSlash(rel)); problem != nil {
				return nil, problem
			}
			supported, problem := matchesSupportedTarget(p.Root, filepath.ToSlash(rel), sdk)
			if problem != nil {
				return nil, problem
			}
			if !supported {
				continue
			}
			parsed, err := parser.ParseFile(token.NewFileSet(), filename, data, parser.ImportsOnly)
			if err != nil {
				return nil, fail("object_extraction_failed", err.Error(), filepath.ToSlash(rel))
			}
			for _, imported := range parsed.Imports {
				name, err := strconv.Unquote(imported.Path.Value)
				if err != nil {
					return nil, fail("object_extraction_failed", err.Error(), filepath.ToSlash(rel))
				}
				dependency, local, err := p.importDirectory(name)
				if err != nil {
					return nil, fail("unsupported_configuration", err.Error(), filepath.ToSlash(rel))
				}
				if local {
					if err := within(p.Root, dependency); err != nil {
						return nil, fail("unsupported_configuration", err.Error(), filepath.ToSlash(rel))
					}
					queue = append(queue, dependency)
				}
			}
		}
	}
	sort.Strings(files)
	return files, nil
}

// sourcePaths 在每轮语义准入前取快照，保留旧配置来源随加载顺序增长的证据。
func (s *sourceAdmission) sourcePaths() []string {
	p := s.Project
	paths := []string{}
	if p.WorkSource != "" {
		paths = append(paths, p.WorkSource)
	}
	for _, m := range p.Modules {
		paths = append(paths, m.Source)
	}
	for _, m := range s.ContextModules {
		paths = append(paths, m.Source)
	}
	sort.Strings(paths)
	return paths
}

// admitSemanticImports 按实际目标遍历 SDK 与仓内混合导入，补齐配置发现之外的包语言索引。
func (s *sourceAdmission) admitSemanticImports(sdk platform, packages []string, includeTests bool) *analysisError {
	p := s.Project
	target := standardTarget(sdk)
	selected := map[string]bool{}
	for _, name := range packages {
		selected[name] = true
	}
	queue := append([]string{}, packages...)
	visited := map[string]bool{}
packageLoop:
	for len(queue) > 0 {
		if problem := p.checkpoint(); problem != nil {
			return problem
		}
		name := queue[0]
		queue = queue[1:]
		if visited[name] || name == "unsafe" && !selected[name] || name == "C" || path.Clean(name) != name || strings.HasPrefix(name, "/") || strings.Contains(name, "\\") {
			continue
		}
		visited[name] = true
		dir, local, err := p.importDirectory(name)
		if err != nil {
			return fail("unsupported_configuration", err.Error())
		}
		allowed := p.Root
		if !local {
			first := strings.Split(name, "/")[0]
			if strings.Contains(first, ".") || first == "cmd" {
				continue
			}
			allowed = filepath.Join(sdk.Root, "src")
			dir = filepath.Join(allowed, filepath.FromSlash(name))
		}
		if err := within(allowed, dir); err != nil {
			if local {
				return fail("object_extraction_failed", err.Error())
			}
			continue
		}
		tests := includeTests && selected[name]
		if local && s.Packages[dir] == nil {
			if problem := s.validateSemanticSources(dir, tests); problem != nil {
				return problem
			}
		}
		meta, err := target.ImportDir(dir, build.IgnoreVendor)
		if err != nil {
			if local {
				return fail("object_extraction_failed", fmt.Sprintf("package %s: %v", name, err))
			}
			continue
		}
		files := append([]string{}, meta.GoFiles...)
		if tests {
			files = append(files, meta.TestGoFiles...)
		}
		sort.Strings(files)
		imports := []string{}
		for _, filename := range files {
			if problem := p.checkpoint(); problem != nil {
				return problem
			}
			filename = filepath.Join(dir, filename)
			data, err := safeFile(allowed, filename)
			if err != nil {
				if local {
					return fail("object_extraction_failed", err.Error())
				}
				continue packageLoop
			}
			parsed, err := parser.ParseFile(token.NewFileSet(), filename, data, parser.SkipObjectResolution)
			if err != nil {
				if local {
					return fail("object_extraction_failed", err.Error())
				}
				continue packageLoop
			}
			for _, imported := range parsed.Imports {
				value, _ := strconv.Unquote(imported.Path.Value)
				imports = append(imports, value)
			}
		}
		if local {
			m, err := s.registerPackage(dir)
			if err != nil {
				return fail("unsupported_configuration", err.Error())
			}
			if problem := m.validateSDKVersion(sdk); problem != nil {
				return problem
			}
		}
		queue = append(queue, imports...)
	}
	return nil
}

// validateSemanticSources 保留原 importer 对全部仓内候选源码的路径和标签检查，不扩展配置目标。
func (s *sourceAdmission) validateSemanticSources(dir string, includeTests bool) *analysisError {
	p := s.Project
	entries, err := os.ReadDir(dir)
	if err != nil {
		return fail("object_extraction_failed", err.Error())
	}
	files := []string{}
	for _, entry := range entries {
		if problem := p.checkpoint(); problem != nil {
			return problem
		}
		if entry.IsDir() || !goSourceName(entry.Name()) || !includeTests && strings.HasSuffix(entry.Name(), "_test.go") {
			continue
		}
		filename := filepath.Join(dir, entry.Name())
		if _, err := safeFile(p.Root, filename); err != nil {
			return fail("object_extraction_failed", err.Error())
		}
		rel, _ := filepath.Rel(p.Root, filename)
		files = append(files, filepath.ToSlash(rel))
	}
	for _, rel := range files {
		if problem := p.checkpoint(); problem != nil {
			return problem
		}
		if _, problem := hasTargetConstraint(p.Root, rel); problem != nil {
			return problem
		}
	}
	return nil
}
