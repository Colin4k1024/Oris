package evolution

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"regexp"
	"strings"
)

var (
	spacePattern = regexp.MustCompile(`\s+`)
	digitPattern = regexp.MustCompile(`\b\d+\b`)
	pathPattern  = regexp.MustCompile(`([A-Za-z]:)?[/\\][^\s:]+`)
)

func DetectSignal(err error, taskClass string, context map[string]any) EvolutionSignal {
	message := ""
	errorType := "error"
	if err != nil {
		message = err.Error()
		errorType = errorTypeFromError(err)
	}
	return NewSignal(taskClass, errorType, message, context)
}

func NewSignal(taskClass string, errorType string, message string, context map[string]any) EvolutionSignal {
	if taskClass == "" {
		taskClass = "general"
	}
	if errorType == "" {
		errorType = "error"
	}
	normalized := NormalizeMessage(message)
	return EvolutionSignal{
		TaskClass:   taskClass,
		ErrorType:   errorType,
		Message:     message,
		Fingerprint: Fingerprint(taskClass, errorType, normalized),
		Context:     context,
	}
}

func NormalizeMessage(message string) string {
	message = strings.TrimSpace(message)
	if idx := strings.IndexByte(message, '\n'); idx >= 0 {
		message = message[:idx]
	}
	message = pathPattern.ReplaceAllString(message, "<path>")
	message = digitPattern.ReplaceAllString(message, "<num>")
	message = spacePattern.ReplaceAllString(message, " ")
	return strings.ToLower(strings.TrimSpace(message))
}

func Fingerprint(parts ...string) string {
	joined := strings.Join(parts, "|")
	sum := sha256.Sum256([]byte(joined))
	return hex.EncodeToString(sum[:])[:24]
}

func errorTypeFromError(err error) string {
	if err == nil {
		return "error"
	}
	var target interface{ Type() string }
	if errors.As(err, &target) && target.Type() != "" {
		return target.Type()
	}
	name := strings.TrimPrefix(strings.TrimPrefix(strings.TrimPrefix(strings.TrimSpace(strings.SplitN(err.Error(), ":", 2)[0]), "*"), "&"), " ")
	if name == "" || strings.Contains(name, " ") {
		return "error"
	}
	return strings.ToLower(name)
}
