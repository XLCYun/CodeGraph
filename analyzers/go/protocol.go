package main

import (
	"encoding/json"
	"reflect"
)

const protocol = "codegraph.analyzer/1"

// request 是一次分析使用的完整协议输入。
type request struct {
	Protocol    string      `json:"protocol"`
	Type        string      `json:"type"`
	ID          string      `json:"id"`
	Input       input       `json:"input"`
	Environment environment `json:"environment"`
	Limits      limits      `json:"limits"`
}

// input 描述固定侧仓库和选中文件。
type input struct {
	RepositoryRoot string   `json:"repository_root"`
	Files          []string `json:"files"`
	IncludeTests   bool     `json:"include_tests"`
	Profile        string   `json:"profile"`
}

// environment 只包含受批准的平台声明和隔离 scratch 目录。
type environment struct {
	ScratchRoot string     `json:"scratch_root"`
	Platforms   []platform `json:"platforms"`
}

// platform 指定目标 Go SDK 与一个目标系统架构。
type platform struct {
	ID       string           `json:"id"`
	Kind     string           `json:"kind"`
	Version  string           `json:"version"`
	Root     string           `json:"root"`
	Metadata platformMetadata `json:"metadata"`
	ToolTags []string         `json:"-"`
}

// platformMetadata 明确列出受支持的目标维度。
type platformMetadata struct {
	GOOS    string `json:"goos"`
	GOARCH  string `json:"goarch"`
	GOAMD64 string `json:"goamd64,omitempty"`
}

// limits 限制单次分析的时间和输出大小。
type limits struct {
	AnalysisTimeoutMS  int `json:"analysis_timeout_ms"`
	MaxResultBytes     int `json:"max_result_bytes"`
	MaxDiagnosticBytes int `json:"max_diagnostic_bytes"`
}

// node 是公共图中一个有单一声明父级的源码对象。
type node struct {
	Category   string         `json:"category"`
	ID         string         `json:"id"`
	Language   string         `json:"language"`
	NativeKind string         `json:"native_kind"`
	Name       *string        `json:"name"`
	Signature  *string        `json:"signature"`
	Code       *string        `json:"code"`
	Source     *string        `json:"source"`
	Contains   []string       `json:"contains"`
	References []reference    `json:"references"`
	Extensions map[string]any `json:"extensions"`
}

// reference 是一个已确认或保守未知的关系观察。
type reference struct {
	Semantic string `json:"semantic"`
	Target   target `json:"target"`
}

// target 用 kind 区分图内、图外和未知目标。
type target struct {
	Kind    string `json:"kind"`
	ID      string `json:"id,omitempty"`
	Symbol  string `json:"symbol,omitempty"`
	Name    string `json:"name,omitempty"`
	Scope   string `json:"-"`
	Display string `json:"-"`
}

// externalSymbol 描述图外但已确认身份的仓内或 SDK 符号。
type externalSymbol struct {
	Symbol string `json:"symbol"`
	Name   string `json:"name"`
	Scope  string `json:"scope"`
}

// graph 是 helper 已完成的图构造数据。
type graph struct {
	RootContains    []string         `json:"root_contains"`
	Nodes           []node           `json:"nodes"`
	ExternalSymbols []externalSymbol `json:"external_symbols"`
}

// configurationEvidence 记录实际分析的配置及来源。
type configurationEvidence struct {
	ID          string   `json:"id"`
	SourcePaths []string `json:"source_paths"`
	PlatformIDs []string `json:"platform_ids"`
}

// fileEvidence 记录输入文件在配置中的覆盖结果。
type fileEvidence struct {
	Path             string   `json:"path"`
	Disposition      string   `json:"disposition"`
	ConfigurationIDs []string `json:"configuration_ids"`
}

// originEvidence 记录图对象被哪些选中源码支持。
type originEvidence struct {
	NodeID        string   `json:"node_id"`
	SelectedPaths []string `json:"selected_paths"`
}

// evidence 汇总完整性可交叉检验的证据。
type evidence struct {
	Configurations []configurationEvidence `json:"configurations"`
	Files          []fileEvidence          `json:"files"`
	Origins        []originEvidence        `json:"origins"`
}

// analysisError 是协议允许的原子失败。
type analysisError struct {
	Code    string   `json:"code"`
	Message string   `json:"message"`
	Paths   []string `json:"paths"`
}

// completed 是分析的唯一终结消息。
type completed struct {
	Protocol string         `json:"protocol"`
	Type     string         `json:"type"`
	ID       string         `json:"id"`
	Outcome  string         `json:"outcome"`
	Graph    *graph         `json:"graph,omitempty"`
	Evidence *evidence      `json:"evidence,omitempty"`
	Error    *analysisError `json:"error,omitempty"`
}

// fail 创建带明确路径的原子分析错误。
func fail(code, message string, paths ...string) *analysisError {
	return &analysisError{Code: code, Message: message, Paths: append([]string{}, paths...)}
}

// stringPtr 将值型字符串用于可空的图字段。
func stringPtr(value string) *string { return &value }

// sameNodeContent 比较跨配置同身份对象的稳定自身内容。
func sameNodeContent(a, b node) bool {
	return a.Category == b.Category && a.Language == b.Language && a.NativeKind == b.NativeKind && reflect.DeepEqual(a.Name, b.Name) && reflect.DeepEqual(a.Signature, b.Signature) && reflect.DeepEqual(a.Code, b.Code) && reflect.DeepEqual(a.Source, b.Source) && reflect.DeepEqual(a.Extensions, b.Extensions)
}

// MarshalJSON 保留省略与显式空数组的差别，测试请求也使用同一私有契约。
func (value input) MarshalJSON() ([]byte, error) {
	fields := map[string]any{"repository_root": value.RepositoryRoot, "include_tests": value.IncludeTests, "profile": value.Profile}
	if value.Files != nil {
		fields["files"] = value.Files
	}
	return json.Marshal(fields)
}
