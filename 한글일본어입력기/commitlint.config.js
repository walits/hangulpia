module.exports = {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'type-enum': [2, 'always', [
      'feat', 'fix', 'docs', 'style', 'refactor',
      'perf', 'test', 'build', 'ci', 'chore', 'revert',
    ]],
    'scope-enum': [1, 'always', [
      'core', 'hangul', 'japanese', 'db',
      'macos', 'android', 'linux',
      'ci', 'infra', 'deps', 'scripts',
    ]],
    'header-max-length': [2, 'always', 72],
    'subject-empty': [2, 'never'],
    'type-empty': [2, 'never'],
  },
};
