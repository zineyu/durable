import { useEffect, useMemo, useState } from 'react';
import type { ReactElement } from 'react';
import type { ApiResult } from '../api/client';
import { api } from '../api/client';
import type { HostName } from '../api/generated/HostName';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { AssistantContent } from './AssistantContent';
import { AssistantMessage } from './AssistantMessage';
import { CompactionMessage } from './CompactionMessage';
import { Composer } from './Composer';
import { EmptyState } from './EmptyState';
import { MainHeader } from './MainHeader';
import { MessagePending } from './MessagePending';
import { QueueStrip } from './QueueStrip';
import { SearchBar } from './SearchBar';
import { Transcript } from './Transcript';
import { UserMessage } from './UserMessage';
import { bodyText } from '../view/content';
import { toWorkspacePath } from '../view/ids';
import { searchTranscript, stepHit } from '../view/search';
import { markdownFileName, sessionMarkdown } from '../view/session-markdown';
import type {
  BranchControl,
  BranchPosition,
  ConnectionState,
  HostRecord,
  SessionView,
  TranscriptEntry,
} from '../view/types';

export type SessionPaneProps = {
  host: HostName;
  record: HostRecord;
  view: SessionView;
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
};

/// 本模块负责渲染一个 session 的派生视图：头部、搜索栏、transcript、队列与输入框。
/// 它不读分片，也不判定 session 状态：这些都在 `view/derived.ts`。
export function SessionPane({
  host,
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
}: SessionPaneProps): ReactElement {
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [hitIndex, setHitIndex] = useState(-1);

  const hits = useMemo(
    () => (searchOpen ? searchTranscript(view, query) : []),
    [searchOpen, view, query],
  );
  const activeHit =
    hits.length === 0 ? null : hits[Math.max(0, Math.min(hitIndex, hits.length - 1))] ?? null;

  useEffect(() => {
    if (activeHit === null) {
      return;
    }
    document
      .querySelector(`[data-message-id="${activeHit.messageId}"]`)
      ?.scrollIntoView({ block: 'center' });
  }, [activeHit]);

  const idle = view.idle;
  const results = view.toolResults;
  const sessionId = view.session.id;

  const switchLeaf = (leafId: number): void => {
    runCommand(api.switchActiveLeaf(sessionId, leafId));
  };

  const controls = (branch: BranchPosition | null): { branch?: BranchControl } => {
    if (branch === null) {
      return {};
    }
    const control: BranchControl = { index: branch.index, total: branch.total };
    if (!idle) {
      return { branch: control };
    }
    const previousId = branch.previousId;
    const nextId = branch.nextId;
    if (previousId !== null) {
      control.onPrev = () => { switchLeaf(previousId); };
    }
    if (nextId !== null) {
      control.onNext = () => { switchLeaf(nextId); };
    }
    return { branch: control };
  };

  const renderEntry = (entry: TranscriptEntry): ReactElement => {
    const message = entry.message;
    const parts = entry.parts;
    const highlighted = activeHit?.messageId === message.id;
    if (message.role === 'user') {
      if (parts === null) {
        return <MessagePending key={message.id} parts={message.parts} />;
      }
      const text = bodyText(parts);
      return (
        <UserMessage
          key={message.id}
          messageId={message.id}
          highlighted={highlighted}
          text={text}
          onEdit={() => { onEditMessage(message.id, text); }}
          editDisabled={!idle}
          {...controls(entry.branch)}
        />
      );
    }
    if (message.role === 'compaction') {
      return (
        <CompactionMessage
          key={message.id}
          messageId={message.id}
          highlighted={highlighted}
          parts={parts}
          skeleton={message.parts}
        />
      );
    }
    return (
      <AssistantMessage
        key={message.id}
        messageId={message.id}
        highlighted={highlighted}
        state={entry.state}
        {...(entry.note === null ? {} : { note: entry.note })}
        onRetry={() => { onRetryMessage(message.id); }}
        retryDisabled={!idle}
        {...controls(entry.branch)}
      >
        <AssistantContent parts={parts} skeleton={message.parts} toolResults={results} />
      </AssistantMessage>
    );
  };

  const entries = view.transcript.map(renderEntry);

  const pending = view.placeholders.map((placeholder) => (
    <AssistantMessage key={`pending-${String(placeholder.id)}`} state="streaming">
      <AssistantContent parts={placeholder.parts} skeleton={[]} toolResults={results} />
    </AssistantMessage>
  ));

  const closeSearch = (): void => {
    setSearchOpen(false);
    setQuery('');
    setHitIndex(-1);
  };

  const handleExport = (): void => {
    const content = sessionMarkdown(view, host);
    const url = URL.createObjectURL(new Blob([content], { type: 'text/markdown;charset=utf-8' }));
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = markdownFileName(view.session.title);
    anchor.click();
    setTimeout(() => { URL.revokeObjectURL(url); }, 0);
  };

  return (
    <>
      <MainHeader
        host={host}
        hostStatus={record.status}
        hostDetail={record.detail}
        title={view.summary.title}
        workspacePath={toWorkspacePath(view.session.workspace)}
        state={view.summary.state}
        connection={connection}
        searchOpen={searchOpen}
        onSearch={() => {
          if (searchOpen) {
            closeSearch();
          } else {
            setSearchOpen(true);
          }
        }}
        onExport={handleExport}
      />
      {searchOpen ? (
        <SearchBar
          query={query}
          current={activeHit === null ? -1 : hits.indexOf(activeHit)}
          total={hits.length}
          onQueryChange={(next) => {
            setQuery(next);
            setHitIndex(next.trim().length === 0 ? -1 : 0);
          }}
          onStep={(delta) => { setHitIndex(stepHit(hitIndex, delta, hits.length)); }}
          onClose={closeSearch}
        />
      ) : null}
      <Transcript>
        {entries.length === 0 && pending.length === 0 ? (
          <EmptyState title="还没有消息" description="在下方输入框提交第一条消息。" />
        ) : (
          [...entries, ...pending]
        )}
      </Transcript>
      <QueueStrip
        items={view.queue}
        onRemove={(viewId) => { runCommand(api.removeQueueItem(sessionId, Number(viewId))); }}
      />
      <Composer
        value={draft}
        onChange={onDraftChange}
        onSend={onSend}
        busy={view.session.busy}
        mode={view.session.busy ? sendMode : 'queue'}
        onModeChange={onModeChange}
        {...(view.session.busy ? { onCancel: onCancelRun } : {})}
        history={view.history}
        pastes={pastes}
      />
    </>
  );
}
