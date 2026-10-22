package main

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
)

// TestDirectoryDiscovery 证明根工作区成员、包边界和 replacement 上下文不混为选集。
func TestDirectoryDiscovery(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go "+fixtureLanguage(t)+"\nuse (\n ./app\n ./lib\n)\n")
	writeFixture(t, root, "app/go.mod", "module example.test/app\ngo "+fixtureLanguage(t)+"\nreplace example.test/context => ../context\n")
	writeFixture(t, root, "lib/go.mod", "module example.test/lib\ngo "+fixtureLanguage(t)+"\n")
	writeFixture(t, root, "context/go.mod", "module example.test/context\ngo "+fixtureLanguage(t)+"\n")
	writeFixture(t, root, "context/api.go", "package context\nfunc Call(){}\n")
	writeFixture(t, root, "app/main.go", "package app\nimport \"example.test/context\"\nfunc Main(){context.Call()}\n")
	writeFixture(t, root, "app/main_test.go", "package app\nfunc TestMain(){Main()}\n")
	writeFixture(t, root, "lib/lib.go", "package lib\nfunc Library(){}\n")
	writeFixture(t, root, "app/child.go/member.go", "package child\nfunc Member(){}\n")
	writeFixture(t, root, "app/future.go", "//go:build go1.999\n\npackage app\nfunc Future(){}\n")
	for _, file := range []string{"app/vendor/bad/bad.go", "app/testdata/bad.go", "app/.hidden/bad.go", "app/_ignored/bad.go", "app/_ignored.go"} {
		writeFixture(t, root, file, "invalid Go deliberately excluded")
	}
	writeFixture(t, root, "app/nested/go.mod", "module example.test/nested\ngo "+fixtureLanguage(t)+"\n")
	writeFixture(t, root, "app/nested/ignored.go", "invalid Go deliberately excluded")
	req := fixtureRequest(t, root)
	req.Environment.Platforms = req.Environment.Platforms[2:]
	before := snapshotFixture(t, root)
	result := analyze(context.Background(), req)
	if result.Outcome != "ok" {
		t.Fatalf("discovery: %+v", result.Error)
	}
	coverage := map[string]string{}
	for _, file := range result.Evidence.Files {
		coverage[file.Path] = file.Disposition
	}
	want := map[string]string{"app/main.go": "analyzed", "app/main_test.go": "excluded_test", "app/child.go/member.go": "analyzed", "lib/lib.go": "analyzed"}
	if !reflect.DeepEqual(coverage, want) {
		t.Fatalf("members: %v", coverage)
	}
	if findNode(result.Graph, "go:context/api.go#func:Call") != nil || len(result.Graph.ExternalSymbols) == 0 {
		t.Fatal("replacement leaked into selection or lost binding")
	}
	req.Input.IncludeTests = true
	if result := analyze(context.Background(), req); result.Outcome != "ok" || findNode(result.Graph, "go:app/main_test.go#func:TestMain") == nil {
		t.Fatalf("test discovery: %+v", result)
	}
	req.Input.Files = []string{"app/main.go"}
	if result := analyze(context.Background(), req); result.Outcome != "ok" || len(result.Evidence.Files) != 1 {
		t.Fatalf("explicit expanded: %+v", result)
	}
	if !reflect.DeepEqual(before, snapshotFixture(t, root)) {
		t.Fatal("analysis modified project")
	}
}

// TestDiscoveryEmptyInvalidAndCancelled 覆盖空配置与省略、空数组、null 的不同协议含义。
func TestDiscoveryEmptyInvalidAndCancelled(t *testing.T) {
	root := t.TempDir()
	req := fixtureRequest(t, root)
	req.Environment.Platforms = req.Environment.Platforms[2:]
	if result := analyze(context.Background(), req); result.Outcome != "error" {
		t.Fatal("missing config accepted")
	}
	req.Input.Files = []string{}
	req.Environment.Platforms = nil
	if result := analyze(context.Background(), req); result.Outcome != "ok" || len(result.Graph.Nodes) != 0 {
		t.Fatalf("explicit empty: %+v", result)
	}
	encoded, _ := json.Marshal(req)
	if !strings.Contains(string(encoded), `"files":[]`) {
		t.Fatal("empty selection omitted")
	}
	req = fixtureRequest(t, root)
	req.Environment.Platforms = req.Environment.Platforms[2:]
	writeFixture(t, root, "go.mod", "module example.test/empty\ngo "+fixtureLanguage(t)+"\n")
	result := analyze(context.Background(), req)
	if result.Outcome != "ok" || len(result.Evidence.Files) != 0 || len(result.Evidence.Configurations) == 0 {
		t.Fatalf("empty config: %+v", result)
	}
	encoded, _ = json.Marshal(req)
	if strings.Contains(string(encoded), `"files"`) {
		t.Fatal("discovery serialized selection")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := scanConfigContext(ctx, []byte("go 1.23.8\n")); !errors.Is(err, context.Canceled) {
		t.Fatalf("configuration scan ignored cancellation: %v", err)
	}
	if _, err := parseModContext(ctx, root, root+"/go.mod"); !errors.Is(err, context.Canceled) {
		t.Fatalf("module load ignored cancellation: %v", err)
	}
	if result := analyze(ctx, req); result.Outcome != "cancelled" {
		t.Fatalf("discovery cancellation: %+v", result)
	}
	project, problem := loadProjectContext(ctx, root, nil)
	if problem == nil {
		_, problem = discoverProjectFiles(project, false, req.Environment.Platforms)
	}
	if problem == nil || problem.Code != "cancelled" {
		t.Fatalf("loader ignored cancellation: %+v", problem)
	}
	writeFixture(t, root, "go.mod", "invalid")
	if result := analyze(context.Background(), req); result.Outcome != "error" {
		t.Fatal("invalid root config accepted")
	}
}

// TestDiscoveryBuildConditions 发现使用整个支持矩阵，未知标签不隐藏为无源码。
func TestDiscoveryBuildConditions(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/build\ngo "+fixtureLanguage(t)+"\n")
	writeFixture(t, root, "main_linux.go", "package build\nfunc Linux(){}")
	writeFixture(t, root, "main_windows.go", "package build\nfunc Windows(){}")
	req := fixtureRequest(t, root)
	req.Environment.Platforms = req.Environment.Platforms[:2]
	result := analyze(context.Background(), req)
	if result.Outcome != "ok" || len(result.Evidence.Files) != 2 || len(result.Evidence.Configurations) != 2 {
		t.Fatalf("target union: %+v", result)
	}
	writeFixture(t, root, "custom.go", "//go:build custom\n\npackage build")
	if result := analyze(context.Background(), req); result.Error == nil || result.Error.Code != "unsupported_configuration" {
		t.Fatalf("unknown tag: %+v", result)
	}
}
