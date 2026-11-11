// What each provider is called and what it can do. The runner's checks read
// this rather than comparing provider names, and hello hands it to the apps
// (docs/protocol.md, "Providers"), so adding a provider is a row here and
// not a hunt through every `=== 'codex'`.
//
// What's left comparing names is really about one CLI's own files or
// behaviour (copying Claude's transcripts, Codex's model list).
import { experimentOn } from './experiments.ts';
import { PROVIDERS, type Provider, type ProviderInfo } from './protocol.ts';

export const PROVIDER_INFO: Record<Provider, ProviderInfo> = {
  claude: {
    id: 'claude',
    name: 'Claude',
    cliName: 'Claude Code',
    loginCommand: 'claude auth login',
    effort: 'claude',
    autoMode: 'classifier',
    sandbox: true,
    rewind: true,
    smallJobs: true,
    usageReadings: true,
  },
  codex: {
    id: 'codex',
    name: 'Codex',
    cliName: 'Codex',
    loginCommand: 'codex login',
    effort: 'codex',
    autoMode: 'asks',
    sandbox: false,
    rewind: true,
    smallJobs: false,
    usageReadings: true,
  },
  // Over ACP (adapters/acp/): Auto behaves like Ask, and neither sandbox,
  // rewind nor usage readings exist for them.
  cursor: {
    id: 'cursor',
    name: 'Cursor',
    cliName: 'Cursor Agent',
    loginCommand: 'agent login',
    effort: 'none',
    autoMode: 'asks',
    sandbox: false,
    rewind: false,
    // Its read tools work in any mode and nothing turns them off, and a small
    // job's text can come from anywhere.
    smallJobs: false,
    usageReadings: false,
  },
  opencode: {
    id: 'opencode',
    name: 'OpenCode',
    cliName: 'OpenCode',
    loginCommand: 'opencode auth login',
    effort: 'none',
    autoMode: 'asks',
    sandbox: false,
    rewind: false,
    smallJobs: true,
    usageReadings: false,
  },
  // Grok Build's CLI (adapters/grok/, docs/grok-plan.md), behind the grok
  // experiment. Auto and Plan behave like Ask for now, it can't run in the
  // sandbox, and editing a message reverses the thread's diffs rather than
  // using Grok's own rewind.
  grok: {
    id: 'grok',
    name: 'Grok',
    cliName: 'Grok Build',
    loginCommand: 'grok login',
    effort: 'codex',
    autoMode: 'asks',
    sandbox: false,
    rewind: false,
    smallJobs: false,
    usageReadings: false,
  },
};

const ACP: readonly Provider[] = ['cursor', 'opencode'];

// The providers this runner offers: the ACP agents only with the experiment
// on, and Grok only with the grok experiment on, so with them off nothing of
// theirs turns up anywhere.
export function activeProviders(home: string): Provider[] {
  const acp = experimentOn(home, 'acpAgents');
  const grok = experimentOn(home, 'grok');
  return PROVIDERS.filter((p) => (p === 'grok' ? grok : acp || !ACP.includes(p)));
}

// One this build has no row for (a record or another runner's hello from a
// build that has more): it can do nothing, so no check takes it for Claude
// and lets it into something only Claude can do.
function unknownProvider(id: string): ProviderInfo {
  return { id, name: id, cliName: id, loginCommand: '', effort: 'none', autoMode: 'asks', sandbox: false, rewind: false, smallJobs: false, usageReadings: false };
}

// No provider at all is a Claude one, as a record from before Codex is
// (store.ts). hasOwn, as the id can come from another runner and
// "constructor" is no row.
export function providerInfo(provider: string | undefined): ProviderInfo {
  const id = provider || 'claude';
  return Object.hasOwn(PROVIDER_INFO, id) ? PROVIDER_INFO[id as Provider] : unknownProvider(id);
}

// A provider's name in notes.
export function providerName(provider: string | undefined): string {
  return providerInfo(provider).name;
}

// The program's name, for notes about its versions.
export function cliName(provider: string | undefined): string {
  return providerInfo(provider).cliName;
}

export function providersForHello(providers: readonly Provider[] = PROVIDERS): ProviderInfo[] {
  return providers.map((p) => PROVIDER_INFO[p]);
}
