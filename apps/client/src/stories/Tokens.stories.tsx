import type { Meta, StoryObj } from '@storybook/react-vite';
import styles from './Tokens.module.css';

const meta = {
  title: 'Pages/Tokens',
  parameters: { layout: 'fullscreen' },
} satisfies Meta;

export default meta;

type Story = StoryObj<typeof meta>;

type SwatchSpec = { name: string; value: string };

function Swatch({ name, value }: SwatchSpec) {
  return (
    <div className={styles.swatch}>
      <div className={styles.chip} style={{ background: value }} />
      <div className={styles.swatchBody}>
        <p className={styles.swatchName}>{name}</p>
        <p className={styles.swatchValue}>{value}</p>
      </div>
    </div>
  );
}

const semanticColors: SwatchSpec[] = [
  { name: '--color-bg', value: 'var(--color-bg)' },
  { name: '--color-bg-sidebar', value: 'var(--color-bg-sidebar)' },
  { name: '--color-surface-1', value: 'var(--color-surface-1)' },
  { name: '--color-surface-2', value: 'var(--color-surface-2)' },
  { name: '--color-surface-3', value: 'var(--color-surface-3)' },
  { name: '--color-bubble', value: 'var(--color-bubble)' },
  { name: '--color-accent', value: 'var(--color-accent)' },
  { name: '--color-accent-soft', value: 'var(--color-accent-soft)' },
  { name: '--color-primary-fill', value: 'var(--color-primary-fill)' },
  { name: '--color-success', value: 'var(--color-success)' },
  { name: '--color-warn', value: 'var(--color-warn)' },
  { name: '--color-danger', value: 'var(--color-danger)' },
];

const staticColors: SwatchSpec[] = [
  { name: 'neutral-bluish-50', value: 'var(--palette-neutral-bluish-50)' },
  { name: 'neutral-bluish-100', value: 'var(--palette-neutral-bluish-100)' },
  { name: 'neutral-bluish-200', value: 'var(--palette-neutral-bluish-200)' },
  { name: 'neutral-bluish-300', value: 'var(--palette-neutral-bluish-300)' },
  { name: 'neutral-bluish-400', value: 'var(--palette-neutral-bluish-400)' },
  { name: 'neutral-bluish-500', value: 'var(--palette-neutral-bluish-500)' },
  { name: 'neutral-bluish-600', value: 'var(--palette-neutral-bluish-600)' },
  { name: 'neutral-bluish-700', value: 'var(--palette-neutral-bluish-700)' },
  { name: 'neutral-bluish-850', value: 'var(--palette-neutral-bluish-850)' },
  { name: 'neutral-bluish-900', value: 'var(--palette-neutral-bluish-900)' },
  { name: 'neutral-bluish-950', value: 'var(--palette-neutral-bluish-950)' },
  { name: 'neutral-bluish-1000', value: 'var(--palette-neutral-bluish-1000)' },
  { name: 'deepseek-50', value: 'var(--palette-deepseek-50)' },
  { name: 'deepseek-100', value: 'var(--palette-deepseek-100)' },
  { name: 'deepseek-400', value: 'var(--palette-deepseek-400)' },
  { name: 'deepseek-500', value: 'var(--palette-deepseek-500)' },
  { name: 'deepseek-600', value: 'var(--palette-deepseek-600)' },
  { name: 'red-600', value: 'var(--palette-red-600)' },
  { name: 'amber-500', value: 'var(--palette-amber-500)' },
  { name: 'green-500', value: 'var(--palette-green-500)' },
];

const typeSamples = [
  { name: '--text-xl · 21/30', className: styles.typeXl, text: '会话标题与统计数字' },
  { name: '--text-lg · 18/26', className: styles.typeLg, text: '空状态标题' },
  { name: '--text-base · 14/24', className: styles.typeBase, text: '正文、消息与输入内容' },
  { name: '--text-sm · 13/20', className: styles.typeSm, text: '工具行、侧栏 session 标题' },
  { name: '--text-xs · 12/18', className: styles.typeXs, text: '元数据、状态与时间' },
  { name: '--text-2xs · 11/16', className: styles.type2xs, text: '路径、计数与提示' },
];

const spacing = ['--space-1', '--space-2', '--space-3', '--space-4', '--space-5', '--space-6', '--space-7', '--space-8'];

const radii = [
  { name: '--radius-xs · 2', value: 'var(--radius-xs)' },
  { name: '--radius-sm · 4', value: 'var(--radius-sm)' },
  { name: '--radius-md · 8', value: 'var(--radius-md)' },
  { name: '--radius-lg · 12', value: 'var(--radius-lg)' },
  { name: '--radius-xl · 16', value: 'var(--radius-xl)' },
  { name: '--radius-panel · 20', value: 'var(--radius-panel)' },
];

export const Tokens: Story = {
  render: () => (
    <div className={styles.page}>
      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>语义色</h2>
        <div className={styles.swatches}>
          {semanticColors.map((swatch) => (
            <Swatch key={swatch.name} {...swatch} />
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>静态色阶</h2>
        <div className={styles.swatches}>
          {staticColors.map((swatch) => (
            <Swatch key={swatch.name} {...swatch} />
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>字体阶梯</h2>
        <div className={styles.rows}>
          {typeSamples.map((sample) => (
            <div key={sample.name} className={styles.row}>
              <span className={styles.rowLabel}>{sample.name}</span>
              <p className={`${styles.sample} ${sample.className}`}>{sample.text}</p>
            </div>
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>间距阶梯</h2>
        <div className={styles.rows}>
          {spacing.map((token) => (
            <div key={token} className={styles.row}>
              <span className={styles.rowLabel}>{token}</span>
              <div className={styles.spaceBar} style={{ width: `var(${token})` }} />
            </div>
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>圆角</h2>
        <div className={styles.rows}>
          {radii.map((radius) => (
            <div key={radius.name} className={styles.row}>
              <span className={styles.rowLabel}>{radius.name}</span>
              <div className={styles.radiusBox} style={{ borderRadius: radius.value }} />
            </div>
          ))}
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>层级阴影</h2>
        <div className={styles.rows}>
          <div className={styles.row}>
            <span className={styles.rowLabel}>--elevation-panel</span>
            <div className={styles.elevationBox} style={{ boxShadow: 'var(--elevation-panel)' }}>
              panel
            </div>
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>--elevation-prominent</span>
            <div className={styles.elevationBox} style={{ boxShadow: 'var(--elevation-prominent)' }}>
              prominent
            </div>
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>--elevation-soft</span>
            <div className={styles.elevationBox} style={{ boxShadow: 'var(--elevation-soft)' }}>
              soft · composer
            </div>
          </div>
        </div>
      </section>
    </div>
  ),
};
