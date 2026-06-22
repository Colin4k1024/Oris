package einoadapter

import (
	"context"
	"fmt"
	"strings"

	"github.com/Colin4k1024/Oris/sdks/go/evolution"
	"github.com/cloudwego/eino/compose"
)

type Config struct {
	TaskClass string
}

func ToolMiddleware(adapter *evolution.Adapter, cfg Config) compose.ToolMiddleware {
	return compose.ToolMiddleware{
		Invokable: InvokableToolMiddleware(adapter, cfg),
	}
}

func InvokableToolMiddleware(adapter *evolution.Adapter, cfg Config) compose.InvokableToolMiddleware {
	return func(next compose.InvokableToolEndpoint) compose.InvokableToolEndpoint {
		return func(ctx context.Context, input *compose.ToolInput) (*compose.ToolOutput, error) {
			output, err := next(ctx, input)
			if err == nil {
				return output, nil
			}
			if adapter == nil {
				return nil, err
			}

			taskClass := cfg.TaskClass
			if taskClass == "" {
				taskClass = "eino-tool"
			}
			meta := map[string]any{}
			if input != nil {
				meta["tool_name"] = input.Name
				meta["tool_args"] = input.Arguments
				meta["tool_call_id"] = input.CallID
			}

			signal := adapter.Detect(ctx, err, taskClass, meta)
			candidates, selectErr := adapter.Select(ctx, signal)
			if selectErr != nil || len(candidates) == 0 {
				return nil, err
			}

			decision := adapter.Replay(ctx, candidates[0])
			if decision.Mode == evolution.ReplayModeSkip {
				return nil, err
			}
			return &compose.ToolOutput{Result: ReplayMessage(decision.Instructions)}, nil
		}
	}
}

func ReplayMessage(instructions []string) string {
	if len(instructions) == 0 {
		return "No reusable Oris experience matched this tool failure."
	}
	var b strings.Builder
	b.WriteString("Oris found a reusable experience. Apply these steps:")
	for i, step := range instructions {
		b.WriteString(fmt.Sprintf("\n%d. %s", i+1, step))
	}
	return b.String()
}
