package main

import (
	"go/parser"
	"go/token"
	"strconv"
	"strings"
)

// hasSpecialSource 识别真实的 C 导入与 embed 指令，不把字符串和普通注释当作构建语义。
func hasSpecialSource(filename string, source []byte) bool {
	// 此处只检查特殊源码；语法错误仍由后续目标选择和语义加载按原流程报告。
	file, _ := parser.ParseFile(token.NewFileSet(), filename, source, parser.ParseComments|parser.SkipObjectResolution)
	if file == nil {
		return false
	}
	for _, imported := range file.Imports {
		if name, err := strconv.Unquote(imported.Path.Value); err == nil && name == "C" {
			return true
		}
	}
	for _, group := range file.Comments {
		for _, comment := range group.List {
			if suffix, ok := strings.CutPrefix(comment.Text, "//go:embed"); ok && (suffix == "" || suffix[0] == ' ' || suffix[0] == '\t') {
				return true
			}
		}
	}
	return false
}
