package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"
)

const maxPublicBytes = 128 * 1024 * 1024

// publicResult 仅公开固定身份、最终图或稳定错误，不泄露内部来源和 SDK 路径。
type publicResult struct {
	Protocol      string       `json:"protocol"`
	EngineVersion string       `json:"engine_version"`
	PackID        string       `json:"pack_id"`
	Type          string       `json:"type"`
	ID            *string      `json:"id"`
	Outcome       string       `json:"outcome"`
	Graph         *graph       `json:"graph,omitempty"`
	Error         *publicError `json:"error,omitempty"`
}

// publicError 保持公共错误对象的有限字段。
type publicError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

// uniqueValue 校验递归键唯一和深度；Go 默认 JSON 解码本身不会拒绝重复键。
func uniqueValue(decoder *json.Decoder, depth int) error {
	if depth > 64 {
		return errors.New("JSON nesting limit exceeded")
	}
	token, err := decoder.Token()
	if err != nil {
		return err
	}
	delim, container := token.(json.Delim)
	if !container {
		return nil
	}
	if delim != '{' && delim != '[' {
		return errors.New("invalid JSON delimiter")
	}
	seen := map[string]bool{}
	for decoder.More() {
		if delim == '{' {
			key, err := decoder.Token()
			if err != nil {
				return err
			}
			name, ok := key.(string)
			if !ok || seen[name] {
				return errors.New("duplicate JSON field")
			}
			seen[name] = true
		}
		if err := uniqueValue(decoder, depth+1); err != nil {
			return err
		}
	}
	_, err = decoder.Token()
	return err
}

// validEscapes 拒绝未配对的 JSON Unicode 代理项，避免标准库悄悄替换请求身份。
func validEscapes(data []byte) error {
	quoted := false
	for i := 0; i < len(data); i++ {
		if data[i] == '"' {
			quoted = !quoted
			continue
		}
		if !quoted || data[i] != '\\' {
			continue
		}
		i++
		if i >= len(data) {
			return errors.New("incomplete JSON escape")
		}
		if data[i] != 'u' {
			continue
		}
		if i+4 >= len(data) {
			return errors.New("incomplete Unicode escape")
		}
		value, err := strconv.ParseUint(string(data[i+1:i+5]), 16, 16)
		if err != nil {
			return err
		}
		i += 4
		if value >= 0xdc00 && value <= 0xdfff {
			return errors.New("unpaired Unicode surrogate")
		}
		if value >= 0xd800 && value <= 0xdbff {
			if i+6 >= len(data) || data[i+1] != '\\' || data[i+2] != 'u' {
				return errors.New("unpaired Unicode surrogate")
			}
			low, err := strconv.ParseUint(string(data[i+3:i+7]), 16, 16)
			if err != nil || low < 0xdc00 || low > 0xdfff {
				return errors.New("unpaired Unicode surrogate")
			}
			i += 6
		}
	}
	return nil
}

// strictJSON 独立检查 UTF-8、完整单 JSON 和重复键，再交给指定模型解码。
func strictJSON(data []byte, out any) error {
	if !utf8.Valid(data) {
		return errors.New("request must be UTF-8")
	}
	if err := validEscapes(data); err != nil {
		return err
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := uniqueValue(decoder, 0); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return errors.New("trailing JSON value")
	}
	return json.Unmarshal(data, out)
}

// objectFields 使用精确区分大小写的名字，避开 encoding/json 的结构体字段宽松匹配。
func objectFields(data []byte, required []string, optional ...string) (map[string]json.RawMessage, error) {
	var fields map[string]json.RawMessage
	if len(bytes.TrimSpace(data)) == 0 || bytes.TrimSpace(data)[0] != '{' || json.Unmarshal(data, &fields) != nil {
		return nil, errors.New("expected JSON object")
	}
	allowed := map[string]bool{}
	for _, name := range required {
		allowed[name] = true
		if _, ok := fields[name]; !ok {
			return nil, fmt.Errorf("missing field %s", name)
		}
	}
	for _, name := range optional {
		allowed[name] = true
	}
	for name := range fields {
		if !allowed[name] {
			return nil, fmt.Errorf("unknown field %s", name)
		}
	}
	return fields, nil
}

// rawString 不允许 null 或非字符串被转换为字符串零值。
func rawString(raw json.RawMessage) (string, error) {
	var value string
	if len(raw) == 0 || bytes.TrimSpace(raw)[0] != '"' || json.Unmarshal(raw, &value) != nil {
		return "", errors.New("expected string")
	}
	return value, nil
}

// publicPath 校验公共 POSIX 选集语法，不访问项目或 SDK。
func publicPath(value string) bool {
	if value == "" || strings.ContainsAny(value, "\\:\x00") || strings.HasPrefix(value, "/") {
		return false
	}
	for _, part := range strings.Split(value, "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}

// parsePublic 只接收公共请求，返回已经接受的 ID 及内部强类型分析参数。
func parsePublic(data []byte) (request, *string, *analysisError) {
	req := request{Protocol: protocol, Type: "analyze", Input: input{Profile: "go-static-v1"}, Environment: environment{ScratchRoot: os.TempDir()}, Limits: limits{AnalysisTimeoutMS: 1800000, MaxResultBytes: maxPublicBytes, MaxDiagnosticBytes: 262144}}
	invalid := func(err error) (request, *string, *analysisError) {
		return req, nil, fail("invalid_request", err.Error())
	}
	var raw any
	if err := strictJSON(data, &raw); err != nil {
		return invalid(err)
	}
	fields, err := objectFields(data, []string{"protocol", "type", "id", "repository_root", "options"}, "files")
	if err != nil {
		return invalid(err)
	}
	id, err := rawString(fields["id"])
	if err != nil || strings.TrimSpace(id) == "" || len(id) > 1024 || strings.ContainsRune(id, 0) {
		return invalid(errors.New("invalid request id"))
	}
	req.ID = id
	bad := func(code, message string) (request, *string, *analysisError) { return req, &id, fail(code, message) }
	wire, err := rawString(fields["protocol"])
	if err != nil {
		return bad("invalid_request", err.Error())
	}
	if wire != protocol {
		return bad("incompatible_protocol", "unsupported analyzer protocol")
	}
	kind, err := rawString(fields["type"])
	if err != nil || kind != "analyze" {
		return bad("invalid_request", "expected analyze request")
	}
	options, err := objectFields(fields["options"], []string{"include_tests", "rust_dependency_scope"})
	if err != nil {
		return bad("invalid_request", err.Error())
	}
	if string(bytes.TrimSpace(options["include_tests"])) != "true" && string(bytes.TrimSpace(options["include_tests"])) != "false" {
		return bad("invalid_request", "include_tests must be boolean")
	}
	req.Input.IncludeTests = string(bytes.TrimSpace(options["include_tests"])) == "true"
	scope, err := rawString(options["rust_dependency_scope"])
	if err != nil || (scope != "repository-only" && scope != "standard-library" && scope != "all-dependencies") {
		return bad("invalid_request", "invalid rust_dependency_scope")
	}
	root, err := rawString(fields["repository_root"])
	if err != nil || strings.ContainsRune(root, 0) || !filepath.IsAbs(root) {
		return bad("invalid_input", "repository_root must be an absolute directory")
	}
	req.Input.RepositoryRoot = root
	if files, present := fields["files"]; present {
		if len(bytes.TrimSpace(files)) == 0 || bytes.TrimSpace(files)[0] != '[' {
			return bad("invalid_request", "files must be an array")
		}
		var entries []json.RawMessage
		if json.Unmarshal(files, &entries) != nil {
			return bad("invalid_request", "invalid files")
		}
		if len(entries) > maxSelectionFiles {
			return bad("resource_limit", "selection exceeds file limit")
		}
		req.Input.Files = []string{}
		seen := map[string]bool{}
		for _, entry := range entries {
			file, err := rawString(entry)
			if err != nil || !publicPath(file) || seen[file] {
				return bad("invalid_request", "invalid or duplicate selection path")
			}
			seen[file] = true
			req.Input.Files = append(req.Input.Files, file)
		}
	}
	canonical, err := filepath.EvalSymlinks(root)
	if err != nil {
		return bad("invalid_input", "repository_root is unavailable")
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return bad("invalid_input", "repository_root is not a directory")
	}
	executable, err := os.Executable()
	if err != nil {
		return bad("internal_failure", err.Error())
	}
	executable, err = filepath.EvalSymlinks(executable)
	if err != nil {
		return bad("internal_failure", err.Error())
	}
	installation := filepath.Dir(executable)
	if filepath.Base(installation) == "bin" {
		installation = filepath.Dir(installation)
	}
	if pathUnder(canonical, installation) || pathUnder(installation, canonical) {
		return bad("invalid_input", "source overlaps analyzer installation")
	}
	req.Input.RepositoryRoot = canonical
	for _, file := range req.Input.Files {
		if _, err := safeFile(canonical, filepath.Join(canonical, filepath.FromSlash(file))); err != nil {
			return bad("invalid_input", err.Error())
		}
	}
	return req, &id, nil
}

// pathUnder 按组件比较真实路径，兼容 Windows 的盘符和分隔符。
func pathUnder(candidate, root string) bool {
	relative, err := filepath.Rel(root, candidate)
	return err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))
}

// publicCode 将内部语义失败归入稳定的公共错误集合。
func publicCode(code string) string {
	switch code {
	case "invalid_input", "invalid_request", "incompatible_protocol", "resource_limit", "timeout", "io_failure", "internal_failure":
		return code
	default:
		return "analysis_failed"
	}
}

// runProtocol 等待真实 EOF 后在当前 goroutine 完成一次分析，EOF 永不触发取消。
func runProtocol(in io.Reader, out io.Writer) error {
	result := publicResult{Protocol: protocol, EngineVersion: "0.1.1", PackID: "go", Type: "completed", Outcome: "error"}
	data, err := io.ReadAll(io.LimitReader(in, maxPublicBytes+1))
	if err != nil {
		result.Error = &publicError{"io_failure", err.Error()}
	} else if len(data) > maxPublicBytes {
		result.Error = &publicError{"resource_limit", "request exceeds byte limit"}
	} else {
		req, id, problem := parsePublic(data)
		result.ID = id
		if problem == nil && !(req.Input.Files != nil && len(req.Input.Files) == 0) {
			req.Environment.Platforms, problem = discoverInstalledSDKs()
			if problem == nil {
				for _, sdk := range req.Environment.Platforms {
					if pathUnder(sdk.Root, req.Input.RepositoryRoot) || pathUnder(req.Input.RepositoryRoot, sdk.Root) {
						problem = fail("invalid_input", "target SDK overlaps source repository")
						break
					}
				}
			}
		}
		if problem == nil {
			response := analyze(context.Background(), req)
			if response.Outcome == "ok" {
				problem = validateOutput(req, response)
				if problem == nil {
					result.Outcome = "ok"
					result.Graph = response.Graph
				}
			} else {
				problem = response.Error
				if problem == nil {
					problem = fail("internal_failure", "analysis returned no final result")
				}
			}
		}
		if problem != nil {
			result.Error = &publicError{publicCode(problem.Code), problem.Message}
		}
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		return err
	}
	if len(encoded)+1 > maxPublicBytes {
		result.Outcome = "error"
		result.Graph = nil
		result.Error = &publicError{"resource_limit", "result exceeds byte limit"}
		encoded, err = json.Marshal(result)
		if err != nil {
			return err
		}
	}
	if _, err = out.Write(append(encoded, '\n')); err != nil {
		return err
	}
	if result.Outcome != "ok" {
		return errors.New("analysis failed")
	}
	return nil
}

// runCLI 只接受固定文本选项或唯一 analyze 子命令。
func runCLI(args []string, in io.Reader, out, diagnostic io.Writer) int {
	if len(args) == 1 && (args[0] == "--help" || args[0] == "--version") || len(args) == 2 && args[0] == "analyze" && args[1] == "--help" {
		if args[0] == "--version" {
			fmt.Fprintln(out, "codegraph-go 0.1.1")
		} else {
			fmt.Fprintln(out, "Usage: codegraph-go analyze\nReads one UTF-8 JSON request followed by EOF.")
		}
		return 0
	}
	if len(args) != 1 || args[0] != "analyze" {
		fmt.Fprintln(diagnostic, "Usage: codegraph-go analyze")
		return 2
	}
	if err := runProtocol(in, out); err != nil {
		return 1
	}
	return 0
}

// main 不启动协议监听器；调用方终止本进程即终止 Go 分析。
func main() { os.Exit(runCLI(os.Args[1:], os.Stdin, os.Stdout, os.Stderr)) }
