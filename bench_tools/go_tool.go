package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"strings"
)

type Request struct {
	ID     json.RawMessage `json:"id"`
	Params struct {
		Name      string          `json:"name"`
		Arguments json.RawMessage `json:"arguments"`
	} `json:"params"`
}

type TextInput struct {
	Text string `json:"text"`
}

type Result struct {
	WordCount int `json:"word_count"`
	CharCount int `json:"char_count"`
	LineCount int `json:"line_count"`
}

type Response struct {
	JsonRPC string          `json:"jsonrpc"`
	Result  Result          `json:"result"`
	ID      json.RawMessage `json:"id"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 1024*1024), 1024*1024)
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			continue
		}
		var req Request
		if err := json.Unmarshal([]byte(line), &req); err != nil {
			fmt.Fprintf(os.Stderr, "parse error: %v\n", err)
			continue
		}
		var input TextInput
		json.Unmarshal(req.Params.Arguments, &input)

		text := input.Text
		wordCount := len(strings.Fields(text))
		charCount := len(text)
		lineCount := 0
		if len(text) > 0 {
			lineCount = strings.Count(text, "\n") + 1
		}

		resp := Response{
			JsonRPC: "2.0",
			Result:  Result{wordCount, charCount, lineCount},
			ID:      req.ID,
		}
		out, _ := json.Marshal(resp)
		fmt.Println(string(out))
	}
}
