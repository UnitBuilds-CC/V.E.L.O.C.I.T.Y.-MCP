package main

import (
	"encoding/json"
	"math"
	"regexp"
	"strconv"
	"strings"
	"unsafe"
)

// GenericToolPayload holds the incoming payload with _tool_name and arbitrary args
type GenericToolPayload struct {
	ToolName string                 `json:"_tool_name"`
	Args     map[string]interface{} `json:"-"` // Will be populated from remaining fields
}

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

// LogAnalysisResult holds log analysis metrics
type LogAnalysisResult struct {
	TotalLines      int     `json:"total_lines"`
	ErrorCount      int     `json:"error_count"`
	WarningCount    int     `json:"warning_count"`
	InfoCount       int     `json:"info_count"`
	AvgResponseTime float64 `json:"avg_response_time_ms"`
	SlowestRequest  int     `json:"slowest_request_ms"`
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
	
	// Parse as generic map first to extract _tool_name
	var payloadMap map[string]interface{}
	if err := json.Unmarshal(inputBytes, &payloadMap); err != nil {
		errResult := map[string]string{"error": "invalid json: " + err.Error()}
		return encodeResult(errResult)
	}

	// Extract _tool_name for dispatch
	toolName, ok := payloadMap["_tool_name"].(string)
	if !ok {
		// Fallback: if no _tool_name, assume legacy go_text_stats behavior
		toolName = "go_text_stats"
	}

	// Remove _tool_name from args before passing to handler
	delete(payloadMap, "_tool_name")

	// Dispatch based on tool name
	switch toolName {
	case "go_text_stats":
		return handleTextStats(payloadMap)
	case "analyze_logs_go":
		return handleAnalyzeLogs(payloadMap)
	default:
		errResult := map[string]string{"error": "unknown tool: " + toolName}
		return encodeResult(errResult)
	}
}

func handleTextStats(args map[string]interface{}) int64 {
	// Extract text field
	textRaw, ok := args["text"]
	if !ok {
		errResult := map[string]string{"error": "missing 'text' argument"}
		return encodeResult(errResult)
	}
	text, ok := textRaw.(string)
	if !ok {
		errResult := map[string]string{"error": "'text' must be a string"}
		return encodeResult(errResult)
	}

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

func handleAnalyzeLogs(args map[string]interface{}) int64 {
	// Extract log_lines field
	logLinesRaw, ok := args["log_lines"]
	if !ok {
		errResult := map[string]string{"error": "missing 'log_lines' argument"}
		return encodeResult(errResult)
	}
	
	// Convert []interface{} to []string
	logLinesSlice, ok := logLinesRaw.([]interface{})
	if !ok {
		errResult := map[string]string{"error": "'log_lines' must be an array"}
		return encodeResult(errResult)
	}
	
	logLines := make([]string, len(logLinesSlice))
	for i, v := range logLinesSlice {
		str, ok := v.(string)
		if !ok {
			errResult := map[string]string{"error": "log_lines elements must be strings"}
			return encodeResult(errResult)
		}
		logLines[i] = str
	}
	
	errorCount := 0
	warningCount := 0
	infoCount := 0
	var responseTimes []int
	
	re := regexp.MustCompile(`response_time=(\d+)ms`)
	
	for _, line := range logLines {
		lower := strings.ToLower(line)
		
		if strings.Contains(lower, "error") || strings.Contains(lower, "exception") {
			errorCount++
		} else if strings.Contains(lower, "warn") {
			warningCount++
		} else if strings.Contains(lower, "info") {
			infoCount++
		}
		
		matches := re.FindStringSubmatch(line)
		if len(matches) > 1 {
			if rt, err := strconv.Atoi(matches[1]); err == nil {
				responseTimes = append(responseTimes, rt)
			}
		}
	}
	
	avgRT := 0.0
	maxRT := 0
	if len(responseTimes) > 0 {
		sum := 0
		for _, rt := range responseTimes {
			sum += rt
			if rt > maxRT {
				maxRT = rt
			}
		}
		avgRT = float64(sum) / float64(len(responseTimes))
	}
	
	result := LogAnalysisResult{
		TotalLines:      len(logLines),
		ErrorCount:      errorCount,
		WarningCount:    warningCount,
		InfoCount:       infoCount,
		AvgResponseTime: math.Round(avgRT*100) / 100,
		SlowestRequest:  maxRT,
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
