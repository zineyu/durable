import type { Meta, StoryObj } from '@storybook/react-vite';
import { AppShell } from '../components/AppShell';
import { AssistantMessage } from '../components/AssistantMessage';
import { CodeBlock } from '../components/CodeBlock';
import { Composer } from '../components/Composer';
import { EmptyState } from '../components/EmptyState';
import { ErrorBanner } from '../components/ErrorBanner';
import { MainHeader } from '../components/MainHeader';
import { QueueStrip } from '../components/QueueStrip';
import { SearchBar } from '../components/SearchBar';
import { Sidebar } from '../components/Sidebar';
import { ToolCard } from '../components/ToolCard';
import { Transcript } from '../components/Transcript';
import { UserMessage } from '../components/UserMessage';
import { codeSample, hosts, modelName, queueItems, workspaces } from '../fixtures/view';
import { toSessionId, toWorkspacePath } from '../view/ids';

const meta = {
  title: 'Pages/Console',
  parameters: { layout: 'fullscreen' },
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

const allSessions = workspaces.flatMap((workspace) => workspace.sessions);

const sidebar = (
  <Sidebar
    hosts={hosts}
    selectedHost="macbook-air-01"
    sessions={allSessions}
    modelName={modelName}
    workspaces={workspaces}
    selectedSessionId={toSessionId('s-1')}
  />
);

const emptySidebar = (
  <Sidebar
    hosts={hosts}
    selectedHost="macbook-air-01"
    sessions={allSessions}
    modelName={modelName}
    workspaces={workspaces}
    selectedSessionId={null}
  />
);

export const Running: Story = {
  render: () => (
    <AppShell sidebar={sidebar}>
      <MainHeader
        host="macbook-air-01"
        hostStatus="ok"
        title="双二进制骨架与工程基线"
        workspacePath={toWorkspacePath('/Users/zine/space/project/durable')}
        state="running"
        connection="connected"
      />
      <Transcript>
        <UserMessage text="把 ui 的静态资源改为内嵌，并确认 SSE 不被缓冲。" />
        <AssistantMessage state="complete">
          <p>先确认 ui 的构建顺序，再验证反代路径。</p>
          <ToolCard
            name="read_file"
            state="succeeded"
            summary="devenv.nix"
            input='{"path": "devenv.nix"}'
            result="scripts.durable-build.exec = ''npm run build''"
            durationLabel="8ms"
          />
          <CodeBlock code={codeSample} language="rust" />
        </AssistantMessage>
        <AssistantMessage state="streaming">
          <p>接下来检查生成的协议类型与提交结果是否一致，然后确认 CI 的</p>
        </AssistantMessage>
      </Transcript>
      <QueueStrip items={queueItems} />
      <Composer
        busy
        value="把 health 端点补一个回环地址测试。"
        onChange={() => undefined}
        onCancel={() => undefined}
      />
    </AppShell>
  ),
};

export const Idle: Story = {
  render: () => (
    <AppShell sidebar={sidebar}>
      <MainHeader
        host="macbook-air-01"
        hostStatus="ok"
        title="类型生成一致性检查"
        workspacePath={toWorkspacePath('/Users/zine/space/project/durable')}
        state="idle"
        connection="connected"
      />
      <Transcript>
        <UserMessage text="检查 ts-rs 的输出是否与前端消费的类型一致。" />
        <AssistantMessage state="complete">
          <p>生成文件已提交，CI 会在重新生成后比较差异。</p>
          <CodeBlock code="cargo run -p agent --bin export-types" language="bash" />
        </AssistantMessage>
      </Transcript>
      <Composer value="" onChange={() => undefined} />
    </AppShell>
  ),
};

export const SearchOpen: Story = {
  render: () => (
    <AppShell sidebar={sidebar}>
      <MainHeader
        host="macbook-air-01"
        hostStatus="ok"
        title="SQLite 迁移与恢复"
        workspacePath={toWorkspacePath('/Users/zine/space/project/durable')}
        state="idle"
        connection="connected"
        searchOpen
        onSearch={() => undefined}
        onExport={() => undefined}
      />
      <SearchBar
        query="迁移"
        current={1}
        total={3}
        onQueryChange={() => undefined}
        onStep={() => undefined}
        onClose={() => undefined}
      />
      <Transcript>
        <UserMessage text="实现迁移并在重启后恢复队列。" />
        <AssistantMessage state="complete">
          <p>迁移已经完成，接下来需要补恢复路径的测试。</p>
        </AssistantMessage>
      </Transcript>
      <Composer value="" onChange={() => undefined} />
    </AppShell>
  ),
};

export const Failed: Story = {
  render: () => (
    <AppShell sidebar={sidebar}>
      <ErrorBanner
        code="run_in_progress"
        message="session 有 run 进行中：分支操作需要 session 空闲且队列为空。"
      />
      <MainHeader
        host="macbook-air-01"
        hostStatus="ok"
        title="SQLite 迁移与恢复"
        workspacePath={toWorkspacePath('/Users/zine/space/project/agent-lab')}
        state="idle"
        connection="reconnecting"
      />
      <Transcript>
        <UserMessage text="实现迁移并在重启后恢复队列。" />
        <AssistantMessage state="failed" note="模型上游错误：连接被拒绝。队列将继续执行下一项。">
          <p>迁移已经完成，接下来需要补</p>
        </AssistantMessage>
        <AssistantMessage state="cancelled" note="已取消，收到的部分输出保留在 transcript 中。">
          <p>队列恢复需要在启动时把 running 标记为</p>
        </AssistantMessage>
      </Transcript>
      <QueueStrip items={queueItems} />
      <Composer value="" onChange={() => undefined} disabled />
    </AppShell>
  ),
};

export const NoSession: Story = {
  render: () => (
    <AppShell sidebar={emptySidebar}>
      <MainHeader
        host="macbook-air-01"
        hostStatus="ok"
        title="未选择 session"
        workspacePath={toWorkspacePath('/Users/zine/space/project/durable')}
        state="idle"
        connection="disconnected"
      />
      <Transcript>
        <AssistantMessage state="complete">
          <p>从左侧选择 session，或者注册新的 workspace 后创建 session。</p>
        </AssistantMessage>
      </Transcript>
      <Composer value="" onChange={() => undefined} disabled />
    </AppShell>
  ),
};

export const UnavailableHost: Story = {
  render: () => (
    <AppShell
      sidebar={
        <Sidebar
          hosts={hosts}
          selectedHost="studio"
          sessions={[]}
          notice="连接 host 失败: 隧道已断开"
          commandsDisabled
          modelName=""
          workspaces={[]}
          selectedSessionId={null}
        />
      }
    >
      <EmptyState title="host 不可用" description="连接 host 失败: 隧道已断开" />
    </AppShell>
  ),
};

export const UnknownSession: Story = {
  render: () => (
    <AppShell sidebar={emptySidebar}>
      <EmptyState
        title="未知 session"
        description="session s-404 不在当前状态里。它可能已被删除，或属于尚未连上的 host。"
      />
    </AppShell>
  ),
};

export const NoHosts: Story = {
  render: () => (
    <AppShell
      sidebar={
        <Sidebar
          hosts={[]}
          selectedHost={null}
          sessions={[]}
          modelName=""
          workspaces={[]}
          selectedSessionId={null}
          commandsDisabled
        />
      }
    >
      <EmptyState
        title="选择一个 session"
        description="从左侧选择 session，或者注册新的 host 与 workspace。"
      />
    </AppShell>
  ),
};
