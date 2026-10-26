import type { ReactElement } from 'react';
import type { ApiResult } from '../api/client';
import type { NodeName } from '../api/generated/NodeName';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { EmptyState } from './EmptyState';
import { SessionPane } from './SessionPane';
import { nodeStatusLabel } from '../view/labels';
import type { ConnectionState, NodeRecord, SessionView } from '../view/types';

export type MainPaneProps = {
  node: NodeName | null;
  record: NodeRecord | undefined;
  view: SessionView | null;
  connection: ConnectionState;
  draft: string;
  pastes: Record<string, string>;
  onDraftChange: (text: string, pastes: Record<string, string>) => void;
  onSend: (text: string) => void;
  onCancelRun: () => void;
  sendMode: SubmitMode;
  onModeChange: (mode: SubmitMode) => void;
  runCommand: (result: Promise<ApiResult<unknown>>) => void;
  onEditMessage: (messageId: number, text: string) => void;
  onRetryMessage: (messageId: number) => void;
  onRegisterWorkspace: () => void;
};

/// 本模块负责主面板的分支选择：未选节点、未知节点、节点不可用、未选 session
/// 与 session 本体。session 的渲染在 `SessionPane.tsx`，派生视图在 `view/derived.ts`。
export function MainPane({
  node,
  record,
  view,
  connection,
  draft,
  pastes,
  onDraftChange,
  onSend,
  onCancelRun,
  sendMode,
  onModeChange,
  runCommand,
  onEditMessage,
  onRetryMessage,
  onRegisterWorkspace,
}: MainPaneProps): ReactElement {
  if (node === null) {
    return <EmptyState title="选择一个节点" description="在左侧节点切换器中选择一台机器。" />;
  }
  if (record === undefined) {
    return (
      <EmptyState
        title="未知节点"
        description={`节点 ${node} 不在节点表里。它可能已被删除或改名。`}
      />
    );
  }
  if (record.status !== 'ok') {
    return (
      <EmptyState title="节点不可用" description={record.detail ?? nodeStatusLabel[record.status]} />
    );
  }
  if (view === null) {
    return (
      <EmptyState
        title="选择一个 session"
        description="从左侧选择 session，或者注册新的 workspace 后创建 session。"
        actionLabel="注册 workspace"
        onAction={onRegisterWorkspace}
      />
    );
  }
  return (
    <SessionPane
      key={view.session.id}
      node={record.name}
      record={record}
      view={view}
      connection={connection}
      draft={draft}
      pastes={pastes}
      onDraftChange={onDraftChange}
      onSend={onSend}
      onCancelRun={onCancelRun}
      sendMode={sendMode}
      onModeChange={onModeChange}
      runCommand={runCommand}
      onEditMessage={onEditMessage}
      onRetryMessage={onRetryMessage}
    />
  );
}
