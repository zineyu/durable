export default {
  extends: ['stylelint-config-standard'],
  ignoreFiles: ['dist/**', 'storybook-static/**'],
  rules: {
    'declaration-property-value-disallowed-list': [
      {
        '/^color$/': ['/^#/', '/^rgb/', '/^hsl/'],
        'background-color': ['/^#/', '/^rgb/', '/^hsl/'],
        'border-color': ['/^#/', '/^rgb/', '/^hsl/'],
      },
      { message: '颜色必须使用 tokens.css 的语义 token：var(--…)。' },
    ],
    'custom-property-pattern': null,
    'selector-class-pattern': null,
    'custom-property-empty-line-before': null,
    'value-keyword-case': ['lower', { ignoreProperties: ['font-family'] }],
  },
};
