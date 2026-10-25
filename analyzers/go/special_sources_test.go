package main

import "testing"

// TestSpecialSourceSyntax 只按真实导入和指令注释拒绝特殊源码，普通文本不改变准入结果。
func TestSpecialSourceSyntax(t *testing.T) {
	cases := []struct {
		name   string
		source string
		reject bool
	}{
		{"embed_string", "package app\nfunc Diagnostic() string { return \"//go:embed\" }\n", false},
		{"raw_string", "package app\nconst Example = `import \"C\"\n//go:embed file.txt`\n", false},
		{"line_comment", "package app\n// Example: //go:embed file.txt; import \"C\"\nfunc Run() {}\n", false},
		{"block_comment", "package app\n/*\n//go:embed file.txt\nimport \"C\"\n*/\nfunc Run() {}\n", false},
		{"directive_prefix", "package app\n//go:embedded is not an embed directive\nfunc Run() {}\n", false},
		{"spaced_comment", "package app\n// go:embed file.txt\nfunc Run() {}\n", false},
		{"embed", "package app\nimport _ \"embed\"\n//go:embed file.txt\nvar File string\n", true},
		{"embed_tab", "package app\nimport _ \"embed\"\n//go:embed\tfile.txt\nvar File string\n", true},
		{"cgo", "package app\nimport \"C\"\n", true},
		{"cgo_group", "package app\nimport (\n\t\"C\"\n)\n", true},
		{"cgo_whitespace", "package app\nimport\t\"C\"\n", true},
		{"cgo_raw_import", "package app\nimport `C`\n", true},
		{"cgo_escaped_import", "package app\nimport \"\\x43\"\n", true},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			writeFixture(t, root, "unit.go", test.source)
			problem := rejectSpecialSources(root, []string{"unit.go"}, false)
			if test.reject {
				if problem == nil || problem.Code != "unsupported_configuration" || problem.Message != "cgo and embed directives are outside the verified static profile" {
					t.Fatalf("special source must retain its configuration failure: %+v", problem)
				}
			} else if problem != nil {
				t.Fatalf("ordinary source text was mistaken for a directive: %+v", problem)
			}
		})
	}
}
