import { describe, expect, it } from 'vitest';

import type { ToolCallRecord } from '../api/generated/ToolCallRecord';
import { toolCardModel } from './tools';

describe('工具卡片归约', () => {
  it('成功结果给出摘要、入参与结果', () => {
    const model = toolCardModel({
      id: 'call_1',
      name: 'read_file',
      arguments: '{"path":"crates/agent/src/sse.rs"}',
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
    const model = toolCardModel({
      id: 'call_2',
      name: 'bash',
      arguments: '{"command":"cargo test","cwd":"client"}',
      outcome: { status: 'failed', code: 'bash_timeout', message: '命令超时' },
    });

    expect(model.state).toBe('failed');
    expect(model.summary).toBe('cargo test');
    expect(model.result).toBe('bash_timeout: 命令超时');
  });

  it('入参不是 JSON 对象时摘要取原文', () => {
    const record: ToolCallRecord = {
      id: 'call_3',
      name: 'bash',
      arguments: '不是 JSON',
      outcome: { status: 'succeeded', content: '' },
    };

    expect(toolCardModel(record).summary).toBe('不是 JSON');
  });
});
