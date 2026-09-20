import { buildNativeTargets, run } from './build-native.mjs';

buildNativeTargets('native');
run('ctest', [
  '--test-dir',
  '.native-cmake',
  '--output-on-failure',
  '-R',
  '^(atlas-shell-exec|atlas-system-info|atlas-filesystem|atlas-git|atlas-capability-loader|atlas-capability-discovery|atlas-capability-executor|atlas-command-runner)$',
]);
