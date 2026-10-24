package main

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
)

// observeDiscovery 只在真实模块模型已加载后发布验收标记，不暂停或控制原生工作。
func observeDiscovery(requestID string) error {
	destination := os.Getenv("CODEGRAPH_TEST_DISCOVERY_PROBE")
	if destination == "" {
		return nil
	}
	bytes, err := json.Marshal(map[string]any{"schema_version": 1, "helper_id": "codegraph-go-engine", "request_id": requestID, "pid": os.Getpid(), "milestone": "module-configurations-loaded"})
	if err != nil {
		return err
	}
	if len(bytes) > 4096 {
		return errors.New("acceptance observation exceeds limit")
	}
	pending := filepath.Join(filepath.Dir(destination), "reached.pending")
	if err := os.Mkdir(pending, 0700); err != nil {
		return err
	}
	defer os.RemoveAll(pending)
	temporary := filepath.Join(pending, "marker.json")
	file, err := os.OpenFile(temporary, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	_, writeErr := file.Write(bytes)
	closeErr := file.Close()
	if writeErr != nil {
		return writeErr
	}
	if closeErr != nil {
		return closeErr
	}
	return os.Link(temporary, destination)
}
