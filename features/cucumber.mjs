// One profile, two apps: TB_APP says which to drive (gpui by default, or
// swift), and a scenario tagged @<app>-pending hasn't been done on that app
// yet. TB_INCLUDE_PENDING=1 runs those too, to see what's left.
//
// Each run writes reports/<app>-<os>.json, which parity.mjs turns into the
// table in docs/gpui-port/PARITY.md.
const app = process.env.TB_APP ?? 'gpui';
const pending = process.env.TB_INCLUDE_PENDING ? '' : ` and not @${app}-pending`;
const os = { darwin: 'mac', win32: 'windows', linux: 'linux' }[process.platform] ?? process.platform;

// Feature files named on the command line (`cucumber-js window.feature:30`) are
// the run: with `paths` set as well, cucumber-js ran every file anyway, and a
// one-scenario check took a whole suite's twenty minutes (10 Nov).
const named = process.argv.slice(2).some((a) => /\.feature(:\d+)*$/.test(a));

export default {
  paths: named ? [] : ['./*.feature'],
  import: ['./support/*.ts', './steps/*.ts'],
  tags: `not @manual${pending}${process.env.TB_TAGS ? ` and (${process.env.TB_TAGS})` : ''}`,
  format: ['progress', `json:./reports/${app}-${os}.json`],
  retry: Number(process.env.TB_RETRY ?? 0),
  // Each scenario's app and mock are stopped, but a run once sat for half an
  // hour after its last scenario with nothing left to do (9 Nov).
  forceExit: true,
};
