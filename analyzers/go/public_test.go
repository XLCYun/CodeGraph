package main

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestPublicConformance 复用七语言公共向量，显式空集不访问 SDK 或项目配置。
func TestPublicConformance(t *testing.T) {
	data, err := os.ReadFile("../common/src/tests/public-conformance.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name      string         `json:"name"`
		Patch     map[string]any `json:"patch"`
		Suffix    string         `json:"suffix"`
		Outcome   string         `json:"outcome"`
		ErrorCode string         `json:"error_code"`
	}
	if err = json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	root := t.TempDir()
	for _, test := range cases {
		t.Run(test.Name, func(t *testing.T) {
			request := map[string]any{"protocol": protocol, "type": "analyze", "id": "公共-é-😀", "repository_root": root, "files": []string{}, "options": map[string]any{"include_tests": false, "rust_dependency_scope": "repository-only"}}
			for name, value := range test.Patch {
				request[name] = value
			}
			encoded, _ := json.Marshal(request)
			var out bytes.Buffer
			err := runProtocol(strings.NewReader(string(encoded)+test.Suffix), &out)
			var result publicResult
			if json.Unmarshal(out.Bytes(), &result) != nil || result.Outcome != test.Outcome || result.Protocol != protocol || result.EngineVersion != "0.1.0" || result.PackID != "go" {
				t.Fatalf("%v: %s", err, out.String())
			}
			if test.ErrorCode != "" && (result.Error == nil || result.Error.Code != test.ErrorCode) {
				t.Fatalf("%s", out.String())
			}
		})
	}
	for _, data := range [][]byte{[]byte(`{"id":"x","id":"y"}`), []byte("{\"x\":\"\xff\"}"), []byte(strings.Repeat("[", 66) + strings.Repeat("]", 66))} {
		var out bytes.Buffer
		if runProtocol(bytes.NewReader(data), &out) == nil || !bytes.Contains(out.Bytes(), []byte(`"code":"invalid_request"`)) {
			t.Fatalf("accepted %q: %s", data, out.String())
		}
	}
}

// TestInstalledSDKCatalog 验证 minor 分组、稳定补丁与预览版过滤沿用原政策。
func TestInstalledSDKCatalog(t *testing.T) {
	root := t.TempDir()
	roots := []string{}
	for _, version := range []string{"1.23.1", "1.23.8", "1.27.1", "1.28rc1"} {
		directory := filepath.Join(root, version)
		writeFixture(t, directory, "VERSION", "go"+version+"\n")
		writeFixture(t, directory, "src/builtin/builtin.go", "")
		writeFixture(t, directory, "src/runtime/runtime.go", "")
		roots = append(roots, directory)
	}
	catalog, problem := sdkCatalog(roots)
	if problem != nil || len(catalog) != 2 || catalog[0].Version != "1.23.8" || catalog[1].Version != "1.27.1" {
		t.Fatalf("%+v %+v", catalog, problem)
	}
}

// TestCLI 验证固定子命令与已移除入口都不进入分析。
func TestCLI(t *testing.T) {
	for _, args := range [][]string{nil, {"__worker"}, {"analyze", "analyze"}, {"--pack", "go"}, {"analyze", "--version"}} {
		var out, err bytes.Buffer
		if got := runCLI(args, strings.NewReader(""), &out, &err); got != 2 || out.Len() != 0 {
			t.Fatalf("%v: %d", args, got)
		}
	}
}

// TestEvidenceDoesNotConsumePublicResultLimit 验证内部来源不会挤占公共 JSON 的结果字节预算。
func TestEvidenceDoesNotConsumePublicResultLimit(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/direct\ngo "+fixtureLanguage(t)+"\n")
	writeFixture(t, root, "unit.go", "package direct\nfunc Run(){}\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	first := analyze(context.Background(), req)
	if first.Outcome != "ok" {
		t.Fatal(first.Error)
	}
	result := publicResult{Protocol: protocol, EngineVersion: "0.1.0", PackID: "go", Type: "completed", ID: &req.ID, Outcome: "ok", Graph: first.Graph}
	wire, _ := json.Marshal(result)
	internal, _ := json.Marshal(first)
	if len(internal) <= len(wire)+1 {
		t.Fatal("fixture must distinguish public bytes from internal evidence")
	}
	req.Limits.MaxResultBytes = len(wire) + 1
	final := analyze(context.Background(), req)
	if final.Outcome != "ok" {
		t.Fatalf("internal evidence consumed public byte budget: %+v", final.Error)
	}
	if problem := validateOutput(req, final); problem != nil {
		t.Fatal(problem)
	}
}
