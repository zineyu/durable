import { useEffect, useMemo, useState } from 'react';
import type { ReactElement } from 'react';
import type { ApiResult } from '../api/client';
import { api } from '../api/client';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { NodeName } from '../api/generated/NodeName';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
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
import { bodyOf, placeholdersOf, toolResults } from '../view/bodies';
import { bodyText } from '../view/content';
import { sessionHistory } from '../view/history';
import { toQueueItemId, toWorkspacePath } from '../view/ids';
import { branchPosition, messageView, sessionQueue, transcriptNodes } from '../view/messages';
import type { NodeState } from '../view/node-state';
import { sessionStateOf } from '../view/labels';
import { searchTranscript, stepHit } from '../view/search';
import { markdownFileName, sessionMarkdown } from '../view/session-markdown';
import type { BranchControl, ConnectionState, NodeRecord } from '../view/types';

export type SessionPaneProps = {
  node: NodeName;
  record: NodeRecord;
  shard: NodeState;
  session: SessionMetadata;
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

export function SessionPane({
  node,
  record,
  shard,
  session,
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
    () => (searchOpen ? searchTranscript(shard, session.id, query) : []),
    [searchOpen, shard, session.id, query],
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

  const idle = !session.busy && session.queueLength === 0;
  const results = toolResults(shard, session.id);

  const controls = (message: MessageSkeleton): { branch?: BranchControl } => {
    const position = branchPosition(shard, session.id, message);
    if (position === null) {
      return {};
    }
    const control: BranchControl = { index: position.index, total: position.total };
    if (!idle) {
      return { branch: control };
    }
    const previousId = position.previousId;
    const nextId = position.nextId;
    if (previousId !== null) {
      control.onPrev = () => { runCommand(api.switchActiveLeaf(node, session.id, previousId)); };
    }
    if (nextId !== null) {
      control.onNext = () => { runCommand(api.switchActiveLeaf(node, session.id, nextId)); };
    }
    return { branch: control };
  };

  const renderEntry = (message: MessageSkeleton): ReactElement => {
    const parts = bodyOf(shard, message.id);
    const highlighted = activeHit?.messageId === message.id;
    if (message.role === 'user') {
      if (parts === null) {
        return <MessagePending key={message.id} parts={message.parts} />;
      }
      return (
        <UserMessage
          key={message.id}
          messageId={message.id}
          highlighted={highlighted}
          text={bodyText(parts)}
          onEdit={() => { onEditMessage(message.id, bodyText(parts)); }}
          editDisabled={!idle}
          {...controls(message)}
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
    const view = messageView(shard, session.id, message);
    return (
      <AssistantMessage
        key={message.id}
        messageId={message.id}
        highlighted={highlighted}
        state={view.state}
        {...(view.note === null ? {} : { note: view.note })}
        onRetry={() => { onRetryMessage(message.id); }}
        retryDisabled={!idle}
        {...controls(message)}
      >
        <AssistantContent parts={parts} skeleton={message.parts} toolResults={results} />
      </AssistantMessage>
    );
  };

  const entries = transcriptNodes(shard, session.id)
    .filter((message) => message.role !== 'tool')
    .map(renderEntry);

  const pending = placeholdersOf(shard, session.id).map((placeholder) => (
    <AssistantMessage key={`pending-${String(placeholder.id)}`} state="streaming">
      <AssistantContent parts={placeholder.parts} skeleton={[]} toolResults={results} />
    </AssistantMessage>
  ));

  const queue = sessionQueue(shard, session.id).map((item) => ({
    id: toQueueItemId(String(item.id)),
    text: item.text,
    pendingSteer: item.pendingSteer,
  }));

  const closeSearch = (): void => {
    setSearchOpen(false);
    setQuery('');
    setHitIndex(-1);
  };

  const handleExport = (): void => {
    const content = sessionMarkdown(shard, session, node);
    const url = URL.createObjectURL(new Blob([content], { type: 'text/markdown;charset=utf-8' }));
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = markdownFileName(session.title);
    anchor.click();
    setTimeout(() => { URL.revokeObjectURL(url); }, 0);
  };

  return (
    <>
      <MainHeader
        node={node}
        nodeStatus={record.status}
        nodeDetail={record.detail}
        title={session.title}
        workspacePath={toWorkspacePath(session.workspace)}
        state={sessionStateOf(session.busy, session.queueLength)}
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
        items={queue}
        onRemove={(viewId) => { runCommand(api.removeQueueItem(node, session.id, Number(viewId))); }}
      />
      <Composer
        value={draft}
        onChange={onDraftChange}
        onSend={onSend}
        busy={session.busy}
        mode={session.busy ? sendMode : 'queue'}
        onModeChange={onModeChange}
        {...(session.busy ? { onCancel: onCancelRun } : {})}
        history={sessionHistory(shard, session.id)}
        pastes={pastes}
      />
    </>
  );
}
