import { useState } from 'react';
import type { FocusEvent, KeyboardEvent, ReactElement } from 'react';
import type { HostName } from '../api/generated/HostName';
import { IconButton } from './IconButton';
import styles from './HostSwitcher.module.css';
import { StatusDot } from './StatusDot';
import { hostNotice, hostStatusLabel, hostStatusTone } from '../view/labels';
import type { SessionId } from '../view/ids';
import type { HostRecord, SessionSummary, StatusTone } from '../view/types';

export type HostSwitcherProps = {
  hosts: HostRecord[];
  sessions: SessionSummary[];
  selectedHost: HostName | null;
  selectedSessionId?: SessionId | null;
  onSelectSession?: (sessionId: SessionId) => void;
  onRegister?: () => void;
  onRename?: (host: HostName) => void;
  onRemove?: (host: HostName) => void;
  onToggleEnabled?: (host: HostRecord) => void;
  onReconnect?: (host: HostName) => void;
  defaultMenuOpen?: boolean;
};

/// host 面板：下拉里按 host 分组列出 session，并给出注册、改名、删除、停用与重连。
/// session 的选中位置由 `App.tsx` 通过 URL 持有，本组件只发出命令。
/// 规格 user story R-46：注册、改名、删除、停用与手动重连都在这个面板里。
export function HostSwitcher({
  hosts,
  sessions,
  selectedHost,
  selectedSessionId = null,
  onSelectSession,
  onRegister,
  onRename,
  onRemove,
  onToggleEnabled,
  onReconnect,
  defaultMenuOpen = false,
}: HostSwitcherProps): ReactElement {
  const [menuOpen, setMenuOpen] = useState(defaultMenuOpen);

  const handleBlur = (event: FocusEvent<HTMLElement>) => {
    if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) {
      return;
    }
    setMenuOpen(false);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === 'Escape') {
      setMenuOpen(false);
    }
  };

  const close = () => { setMenuOpen(false); };

  return (
    <nav
      className={styles.switcher}
      aria-label="host 切换器"
      onBlur={handleBlur}
      onKeyDown={handleKeyDown}
    >
      <div className={styles.bar}>
        <button
          type="button"
          className={styles.trigger}
          aria-haspopup="true"
          aria-expanded={menuOpen}
          onClick={() => { setMenuOpen((current) => !current); }}
        >
          <StatusDot tone={selectedHost === null ? 'idle' : toneOf(hosts, selectedHost)} />
          <span className={styles.name}>{selectedHost ?? 'host'}</span>
        </button>
        <IconButton icon="plus" label="注册 host" onClick={() => onRegister?.()} />
      </div>
      {menuOpen ? (
        <div className={styles.panel}>
          {hosts.length === 0 ? <p className={styles.empty}>还没有 host</p> : null}
          {hosts.map((record) => {
            const label = hostStatusLabel[record.status];
            return (
              <section key={record.name} className={styles.host} aria-label={`host ${record.name}`}>
                <div className={styles.hostHeader}>
                  <StatusDot tone={hostStatusTone[record.status]} />
                  <span className={styles.name}>{record.name}</span>
                  <span className={styles.status}>{label}</span>
                  {record.latencyMs === null ? null : (
                    <span className={styles.latency}>往返 {record.latencyMs} ms</span>
                  )}
                </div>
                <p className={styles.target}>
                  {record.kind === 'ssh' ? record.target : '本机'}
                </p>
                {record.detail === null ? null : (
                  <p className={styles.detail}>{hostNotice(record)}</p>
                )}
                <div className={styles.hostActions}>
                  <button
                    type="button"
                    className={styles.action}
                    onClick={() => { onToggleEnabled?.(record); }}
                  >
                    {record.enabled ? '停用' : '启用'}
                  </button>
                  <button
                    type="button"
                    className={styles.action}
                    onClick={() => { onReconnect?.(record.name); }}
                  >
                    重连
                  </button>
                  <IconButton
                    icon="pencil"
                    label={`重命名 host：${record.name}`}
                    size="sm"
                    onClick={() => { close(); onRename?.(record.name); }}
                  />
                  <IconButton
                    icon="trash"
                    label={`删除 host：${record.name}`}
                    tone="danger"
                    size="sm"
                    onClick={() => { close(); onRemove?.(record.name); }}
                  />
                </div>
                <ul className={styles.sessions}>
                  {sessions.filter((session) => session.host === record.name).length === 0 ? (
                    <li className={styles.empty}>这个 host 没有 session</li>
                  ) : null}
                  {sessions
                    .filter((session) => session.host === record.name)
                    .map((session) => (
                      <li key={session.id}>
                        <button
                          type="button"
                          className={styles.session}
                          aria-current={session.id === selectedSessionId}
                          onClick={() => {
                            close();
                            onSelectSession?.(session.id);
                          }}
                        >
                          {session.title}
                        </button>
                      </li>
                    ))}
                </ul>
              </section>
            );
          })}
        </div>
      ) : null}
    </nav>
  );
}

function toneOf(hosts: HostRecord[], name: HostName): StatusTone {
  const record = hosts.find((entry) => entry.name === name);
  return record === undefined ? 'idle' : hostStatusTone[record.status];
}
