package main

import (
	"context"
	"fmt"
	"go/build"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// analysisPlan 保存完成源码准入后的配置、精确选集和证据，提取无需再登记模块。
type analysisPlan struct {
	Configurations []analysisConfiguration
	Files          []fileEvidence
	Evidence       []configurationEvidence
}

// analysisConfiguration 绑定独立项目的源码索引、唯一 SDK 和实际选中的包及文件。
type analysisConfiguration struct {
	Sources  *sourceAdmission
	SDK      platform
	Packages []string
	Selected map[string]bool
}

// prepareAnalysis 先完成各项目的导入闭包登记，再冻结配置来源和文件角色。
func prepareAnalysis(ctx context.Context, req request) (*analysisPlan, *analysisError) {
	groups, problem := loadProjectSelectionsContext(ctx, req.Input.RepositoryRoot, req.Input.Files)
	if problem != nil {
		return nil, problem
	}
	discovery := req.Input.Files == nil
	if discovery && len(groups) > 0 {
		if err := observeDiscovery(req.ID); err != nil {
			return nil, fail("internal_failure", err.Error())
		}
	}
	plan := &analysisPlan{Files: []fileEvidence{}, Evidence: []configurationEvidence{}}
	files := append([]string{}, req.Input.Files...)
	active := map[string][]string{}
	owned := map[string][]string{}
	for index, group := range groups {
		if discovery {
			group.Files, problem = discoverProjectFiles(group.Project, req.Input.IncludeTests, req.Environment.Platforms)
			if problem != nil {
				return nil, problem
			}
			files = append(files, group.Files...)
			if problem := rejectSpecialSources(group.Project.Root, group.Files, req.Input.IncludeTests); problem != nil {
				return nil, problem
			}
		}
		configs, problem := prepareProjectSelection(group, req.Input.IncludeTests, req.Environment.Platforms)
		if problem != nil {
			return nil, problem
		}
		for _, config := range configs {
			id := "go-" + config.SDK.ID
			if len(groups) > 1 {
				id = fmt.Sprintf("go-%d-%s", index+1, config.SDK.ID)
			}
			// 替换模块已在完整导入闭包中登记，证据不依赖后续 importer 的调用顺序。
			plan.Evidence = append(plan.Evidence, configurationEvidence{ID: id, SourcePaths: config.Sources.sourcePaths(), PlatformIDs: []string{config.SDK.ID}})
			for _, rel := range group.Files {
				owned[rel] = append(owned[rel], id)
				if config.Selected[rel] {
					active[rel] = append(active[rel], id)
				}
			}
			plan.Configurations = append(plan.Configurations, config)
		}
	}
	for _, rel := range files {
		switch ctx.Err() {
		case context.Canceled:
			return nil, fail("cancelled", "Go discovery cancelled")
		case context.DeadlineExceeded:
			return nil, fail("resource_limit", "Go discovery exceeded analysis_timeout_ms")
		}
		disposition := "analyzed"
		ids := active[rel]
		if strings.HasSuffix(rel, "_test.go") && !req.Input.IncludeTests {
			disposition, ids = "excluded_test", owned[rel]
		} else if len(ids) == 0 {
			return nil, fail("target_unavailable", "selected Go file is inactive in every admitted target", rel)
		}
		plan.Files = append(plan.Files, fileEvidence{Path: rel, Disposition: disposition, ConfigurationIDs: ids})
	}
	return plan, nil
}

// prepareProjectSelection 隔离一个项目的准入域，并用完整上下文确定目标配置并集。
func prepareProjectSelection(group projectSelection, includeTests bool, catalog []platform) ([]analysisConfiguration, *analysisError) {
	candidates, problem := selectSDKVersions(group.Project, catalog)
	if problem != nil {
		return nil, problem
	}
	sources := newSourceAdmission(group.Project)
	selected := []string{}
	for _, rel := range group.Files {
		if includeTests || !strings.HasSuffix(rel, "_test.go") {
			selected = append(selected, rel)
		}
	}
	contextFiles, problem := sources.contextFiles(selected, includeTests, candidates[0])
	if problem != nil {
		return nil, problem
	}
	sdks, problem := choosePlatforms(group.Project, selected, contextFiles, candidates)
	if problem != nil {
		return nil, problem
	}
	configs := []analysisConfiguration{}
	for _, sdk := range sdks {
		config := analysisConfiguration{Sources: sources, SDK: sdk, Selected: map[string]bool{}}
		if problem := config.selectPackages(group.Files, includeTests); problem != nil {
			return nil, problem
		}
		configs = append(configs, config)
	}
	return configs, nil
}

// selectPackages 校验选中包的真实源码边界，并保存当前目标下的文件与包成员。
func (c *analysisConfiguration) selectPackages(files []string, includeTests bool) *analysisError {
	p := c.Sources.Project
	target := standardTarget(c.SDK)
	packages := map[string]bool{}
	for _, rel := range files {
		if problem := p.checkpoint(); problem != nil {
			return problem
		}
		if strings.HasSuffix(rel, "_test.go") && !includeTests {
			continue
		}
		filename := filepath.Join(p.Root, filepath.FromSlash(rel))
		m := p.moduleForFile(filename)
		if m == nil {
			return fail("unsupported_configuration", "selected file is outside workspace modules", rel)
		}
		dir := filepath.Dir(filename)
		entries, err := os.ReadDir(dir)
		if err != nil {
			return fail("object_extraction_failed", err.Error(), rel)
		}
		for _, entry := range entries {
			if problem := p.checkpoint(); problem != nil {
				return problem
			}
			if !entry.IsDir() && goSourceName(entry.Name()) {
				if _, err := safeFile(p.Root, filepath.Join(dir, entry.Name())); err != nil {
					return fail("unsupported_configuration", err.Error(), rel)
				}
			}
		}
		suffix, _ := filepath.Rel(m.Dir, dir)
		importPath := m.Path
		if suffix != "." {
			importPath += "/" + filepath.ToSlash(suffix)
		}
		meta, err := target.ImportDir(dir, build.IgnoreVendor)
		if err != nil {
			if _, ok := err.(*build.NoGoError); !ok {
				return fail("object_extraction_failed", err.Error(), rel)
			}
		}
		name := filepath.Base(filename)
		if includeTests && containsString(meta.XTestGoFiles, name) {
			return fail("unsupported_configuration", "external _test package is outside the verified static profile", rel)
		}
		if containsString(meta.GoFiles, name) || includeTests && containsString(meta.TestGoFiles, name) {
			c.Selected[rel], packages[importPath] = true, true
		}
	}
	for importPath := range packages {
		c.Packages = append(c.Packages, importPath)
	}
	sort.Strings(c.Packages)
	return nil
}
