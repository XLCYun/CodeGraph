package main

import (
	"fmt"
	"path/filepath"
	"strings"
)

// validateOutput 在唯一输出边界交叉检查图、选集、配置和来源；不修复数据。
func validateOutput(req request, result completed) *analysisError {
	bad := func(message string) *analysisError { return fail("internal_failure", message) }
	if result.Graph == nil || result.Evidence == nil {
		return bad("missing graph or source evidence")
	}
	g, ev := result.Graph, result.Evidence
	configs := map[string]bool{}
	platforms := map[string]bool{}
	for _, sdk := range req.Environment.Platforms {
		platforms[sdk.ID] = true
	}
	for _, config := range ev.Configurations {
		if config.ID == "" || configs[config.ID] {
			return bad("duplicate or empty configuration id")
		}
		configs[config.ID] = true
		seen := map[string]bool{}
		for _, file := range config.SourcePaths {
			if seen[file] || !publicPath(file) {
				return bad("invalid configuration source")
			}
			seen[file] = true
			if _, err := safeFile(req.Input.RepositoryRoot, filepath.Join(req.Input.RepositoryRoot, filepath.FromSlash(file))); err != nil {
				return bad(err.Error())
			}
		}
		seen = map[string]bool{}
		for _, id := range config.PlatformIDs {
			if seen[id] || !platforms[id] {
				return bad("unknown configuration SDK")
			}
			seen[id] = true
		}
	}
	if req.Input.Files == nil && len(configs) == 0 {
		return bad("discovery lacks project configuration")
	}
	requested := map[string]bool{}
	for _, file := range req.Input.Files {
		requested[file] = true
	}
	files := map[string]string{}
	physical := map[string]bool{}
	if len(ev.Files) > maxSelectionFiles {
		return bad("source coverage exceeds file limit")
	}
	for _, file := range ev.Files {
		if !publicPath(file.Path) || files[file.Path] != "" || req.Input.Files != nil && !requested[file.Path] {
			return bad("invalid or unselected source coverage")
		}
		full := filepath.Join(req.Input.RepositoryRoot, filepath.FromSlash(file.Path))
		if _, err := safeFile(req.Input.RepositoryRoot, full); err != nil {
			return bad(err.Error())
		}
		real, err := filepath.EvalSymlinks(full)
		if err != nil || physical[real] {
			return bad("source coverage contains physical aliases")
		}
		physical[real] = true
		if file.Disposition != "analyzed" && file.Disposition != "excluded_test" && file.Disposition != "inactive" || req.Input.IncludeTests && file.Disposition == "excluded_test" {
			return bad("invalid source disposition")
		}
		if len(file.ConfigurationIDs) == 0 {
			return bad("source lacks configuration")
		}
		seen := map[string]bool{}
		for _, id := range file.ConfigurationIDs {
			if !configs[id] || seen[id] {
				return bad("invalid source configuration")
			}
			seen[id] = true
		}
		files[file.Path] = file.Disposition
	}
	if req.Input.Files != nil && len(files) != len(requested) {
		return bad("source coverage differs from requested selection")
	}
	nodes := map[string]*node{}
	categories := map[string]bool{"container": true, "type": true, "callable": true, "value": true, "execution_region": true, "extension": true}
	for i := range g.Nodes {
		item := &g.Nodes[i]
		if item.ID == "" || nodes[item.ID] != nil || !strings.HasPrefix(item.ID, "go:") || item.Language != "go" || !categories[item.Category] || item.NativeKind == "" || item.Contains == nil || item.References == nil || item.Extensions == nil {
			return bad("invalid graph node")
		}
		nodes[item.ID] = item
	}
	origins := map[string]bool{}
	for _, origin := range ev.Origins {
		if nodes[origin.NodeID] == nil || origins[origin.NodeID] || len(origin.SelectedPaths) == 0 {
			return bad("invalid node origin")
		}
		origins[origin.NodeID] = true
		seen := map[string]bool{}
		for _, file := range origin.SelectedPaths {
			if files[file] != "analyzed" || seen[file] {
				return bad("origin outside analyzed selection")
			}
			seen[file] = true
		}
	}
	if len(origins) != len(nodes) {
		return bad("node/source evidence mismatch")
	}
	external := map[string]bool{}
	for _, item := range g.ExternalSymbols {
		if item.Symbol == "" || external[item.Symbol] || item.Name == "" || item.Scope != "repository" && item.Scope != "dependency" {
			return bad("invalid external symbol")
		}
		external[item.Symbol] = true
	}
	parent := map[string]bool{}
	pending := append([]string{}, g.RootContains...)
	for _, id := range pending {
		if nodes[id] == nil || parent[id] {
			return bad("invalid graph root")
		}
		parent[id] = true
	}
	for _, item := range g.Nodes {
		for _, child := range item.Contains {
			if nodes[child] == nil || parent[child] {
				return bad("invalid containment")
			}
			parent[child] = true
		}
		for _, ref := range item.References {
			if ref.Semantic == "" {
				return bad("empty reference semantic")
			}
			t := ref.Target
			switch t.Kind {
			case "node":
				if nodes[t.ID] == nil || t.Symbol != "" || t.Name != "" {
					return bad("invalid local reference")
				}
			case "external":
				if !external[t.Symbol] || t.ID != "" || t.Name != "" {
					return bad("missing external descriptor")
				}
			case "unresolved":
				if t.Name == "" || t.ID != "" || t.Symbol != "" {
					return bad("invalid unresolved reference")
				}
			default:
				return bad("invalid reference kind")
			}
		}
	}
	visited := map[string]bool{}
	for len(pending) > 0 {
		id := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if visited[id] {
			return bad("containment cycle")
		}
		visited[id] = true
		pending = append(pending, nodes[id].Contains...)
	}
	if len(visited) != len(nodes) {
		return bad(fmt.Sprintf("unreachable graph nodes: %d", len(nodes)-len(visited)))
	}
	return nil
}

// checkControlSize 对实际配置文件保留 1 MiB 上限，不扩大源码读取策略。
func checkControlSize(filename string, size int64) error {
	if (filepath.Ext(filename) == ".mod" || filepath.Ext(filename) == ".work") && size > 1024*1024 {
		return fmt.Errorf("control file exceeds byte limit: %s", filepath.Base(filename))
	}
	return nil
}
