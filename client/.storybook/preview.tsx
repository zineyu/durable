import type { Decorator, Preview } from '@storybook/react-vite';
import { useEffect } from 'react';
import '../src/styles/tokens.css';
import '../src/styles/global.css';
import './frame.css';

const ThemeDecorator: Decorator = (Story, context) => {
  const theme = context.globals.theme === 'dark' ? 'dark' : 'light';
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  const frame = String(context.parameters.frame ?? 'page');
  return <div className={frame}>{Story()}</div>;
};

const preview: Preview = {
  parameters: {
    layout: 'fullscreen',
    controls: { expanded: false },
    frame: 'page',
  },
  globalTypes: {
    theme: {
      description: '主题',
      toolbar: {
        title: '主题',
        icon: 'circlehollow',
        items: [
          { value: 'light', title: '浅色' },
          { value: 'dark', title: '深色' },
        ],
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: { theme: 'light' },
  decorators: [ThemeDecorator],
};

export default preview;
