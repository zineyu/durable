import type { ReactElement } from 'react';
import type { ApiResult } from '../api/client';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { EmptyState } from './EmptyState';
import { SessionPane } from './SessionPane';
import { hostNotice } from '../view/labels';
import type { SessionId } from '../view/ids';
import type { ConnectionState, HostRecord, SessionView } from '../view/types';

export type MainPaneProps = {
  sessionId: SessionId | null;
  view: SessionView | null;
  host: HostRecord | undefined;
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

/// 本模块负责主面板的分支选择：未选 session、未知 session、host 不可用与 session 本体。
/// session 的渲染在 `SessionPane.tsx`，派生视图在 `view/derived.ts`。
/// 规格 user story R-47：host 不可用时显示说明，不显示会话列表。
export function MainPane({
  sessionId,
  view,
  host,
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
  if (sessionId === null) {
    return (
      <EmptyState
        title="选择一个 session"
        description="从左侧选择 session，或者注册新的 workspace 后创建 session。"
        actionLabel="注册 workspace"
        onAction={onRegisterWorkspace}
      />
    );
  }
  if (view === null) {
    return (
      <EmptyState
        title="未知 session"
        description={`session ${sessionId} 不在当前状态里。它可能已被删除，或属于尚未连上的 host。`}
      />
    );
  }
  if (host?.status !== 'ok') {
    const notice = hostNotice(host) ?? 'host 当前不可用';
    return <EmptyState title="host 不可用" description={notice} />;
  }
  return (
    <SessionPane
      key={view.session.id}
      host={host.name}
      record={host}
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
