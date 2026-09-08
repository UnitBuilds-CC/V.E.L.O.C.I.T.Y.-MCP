package main

import (
	"encoding/json"
	"strings"
	"unsafe"
)

// TextAnalysisResult holds the analysis output
type TextAnalysisResult struct {
	WordCount int `json:"word_count"`
	CharCount int `json:"char_count"`
	LineCount int `json:"line_count"`
}

// TextInput holds the input data
type TextInput struct {
	Text string `json:"text"`
}

//export prepare_call
func prepare_call() {
	// No-op for Go — GC manages memory.
	// Exported for ABI compatibility with the Rust/WASM tool.
}

//export tool_execute
func tool_execute(ptr int32, length int32) int64 {
	// Read input from WASM memory
	inputBytes := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(ptr))), length)
	var input TextInput
	if err := json.Unmarshal(inputBytes, &input); err != nil {
		errResult := map[string]string{"error": "invalid json: " + err.Error()}
		return encodeResult(errResult)
	}

	text := input.Text
	wordCount := len(strings.Fields(text))
	charCount := len(text)
	lineCount := 0
	if len(text) > 0 {
		lineCount = strings.Count(text, "\n") + 1
	}

	result := TextAnalysisResult{
		WordCount: wordCount,
		CharCount: charCount,
		LineCount: lineCount,
	}

	return encodeResult(result)
}

func encodeResult(v interface{}) int64 {
	data, err := json.Marshal(v)
	if err != nil {
		data = []byte(`{"error":"marshal failed"}`)
	}

	// Pin the data so GC doesn't move it while the host reads it
	pinned := make([]byte, len(data))
	copy(pinned, data)

	ptr := int64(uintptr(unsafe.Pointer(&pinned[0])))
	length := int64(len(pinned))

	// Encode as (ptr << 32) | length
	return (ptr << 32) | length
}

func main() {}
