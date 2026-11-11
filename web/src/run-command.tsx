// An ask's command and its Run button (docs/protocol.md, "Run on the card"):
// the command in full, as it's what the press agrees to. Pressed through
// phone:press, like a reply button, and it runs on the thread's Mac.
import { useState } from 'react';
import { useMutation } from 'convex/react';
import { api } from '../../convex/_generated/api';

export function RunCommand({ token, threadId, run }: { token: string; threadId: string; run: { command: string; buttonId: string; cwd?: string; running?: boolean; failed?: string } }) {
  const press = useMutation(api.phone.press);
  // From the click until the row says it's going, so a second click can't queue another.
  const [asked, setAsked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const going = asked || run.running === true;
  const failed = error ?? run.failed;
  const onClick = async () => {
    if (going) return;
    setAsked(true);
    setError(null);
    try {
      await press({ token, threadId, buttonId: run.buttonId });
    } catch (e) {
      setError(String((e as { data?: { message?: string } })?.data?.message ?? (e as Error)?.message ?? e));
      setAsked(false);
    }
  };
  return (
    <div className="ask-run">
      <pre className="ask-run-command">{run.command}</pre>
      {run.cwd ? <div className="ask-run-note">In {run.cwd}</div> : null}
      {failed ? <div className="ask-run-note ask-run-failed">{failed}</div> : null}
      <div className="ask-links">
        <button className="btn primary" disabled={going} onClick={() => void onClick()}>
          {going ? <span className="spinner" /> : null}
          <span>{going ? 'Running' : failed ? 'Run again' : 'Run'}</span>
        </button>
      </div>
    </div>
  );
}
