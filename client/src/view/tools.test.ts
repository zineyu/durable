import { describe, expect, it } from 'vitest';

import type { ToolCallPart } from './tools';
import { toolCardModel } from './tools';

function call(id: string, name: string, argumentsText: string): ToolCallPart {
  return { kind: 'tool_call', callId: id, name, arguments: argumentsText, providerMeta: null };
}

describe('工具卡片归约', () => {
  it('成功结果给出摘要、入参与结果', () => {
    const model = toolCardModel(call('call_1', 'read_file', '{"path":"crates/agent/src/sse.rs"}'), {
      status: 'finished',
      outcome: { status: 'succeeded', content: '文件内容' },
    });

    expect(model).toEqual({
      id: 'call_1',
      name: 'read_file',
      state: 'succeeded',
      summary: 'crates/agent/src/sse.rs',
      input: '{"path":"crates/agent/src/sse.rs"}',
      result: '文件内容',
    });
  });

  it('命令摘要取 command，失败结果带错误码与说明', () => {
    const model = toolCardModel(call('call_2', 'bash', '{"command":"cargo test","cwd":"client"}'), {
      status: 'finished',
      outcome: { status: 'failed', code: 'bash_timeout', message: '命令超时' },
    });

    expect(model.state).toBe('failed');
    expect(model.summary).toBe('cargo test');
    expect(model.result).toBe('bash_timeout: 命令超时');
  });

  it('结局未到时按执行中渲染，没有结果正文', () => {
    const pending = toolCardModel(call('call_3', 'read_file', '{"path":"README.md"}'), undefined);
    const running = toolCardModel(call('call_3', 'read_file', '{"path":"README.md"}'), {
      status: 'running',
    });

    expect(pending.state).toBe('running');
    expect(pending.result).toBeNull();
    expect(running).toEqual(pending);
  });

  it('结局丢失观测时给出未知状态与说明', () => {
    const model = toolCardModel(call('call_4', 'bash', '{"command":"cargo test"}'), {
      status: 'unknown',
    });

    expect(model.state).toBe('unknown');
    expect(model.result).toBe('工具结果未知：执行中崩溃，结局没有观测到。');
  });

  it('入参不是 JSON 对象时摘要取原文', () => {
    const model = toolCardModel(call('call_5', 'bash', '不是 JSON'), {
      status: 'finished',
      outcome: { status: 'succeeded', content: '' },
    });

    expect(model.summary).toBe('不是 JSON');
  });
});
