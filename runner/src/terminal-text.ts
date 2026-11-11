// Text bound for a terminal that holds anything another Mac, a runner key, a
// model or Convex wrote (`tb status`'s lines about other runners, thread
// titles): no escape sequences or control characters, so none of it can
// rewrite the screen, set the window's title or the clipboard (OSC 52), or
// hide a line. Daybreak, 10 Nov (finding 76): a runner key's heartbeat put
// ESC ]52 in its followProblem, and `tb status` wrote it out as it was.
// Newlines and tabs stay, so a multi-line report still reads; runner-keys.ts
// has the one-line version for rows.

// A whole sequence, not just its ESC: OSC, DCS, SOS, PM and APC run to BEL or
// ST (ESC \), or the line's end; CSI to its final byte; anything else is ESC and one character.
// The 8-bit C1 forms (U+0090 DCS, U+009B CSI, U+009D OSC...) likewise.
const SEQUENCE = /(?:\u001b[\]PX^_]|[\u0090\u0098\u009d\u009e\u009f])[^\u0007\u001b\u009c\n]*(?:\u0007|\u001b\\|\u009c)?|(?:\u001b\[|\u009b)[0-?]*[ -/]*[@-~]?|\u001b[ -~]?/g;
// What's left: C0 and C1 controls but newline and tab, and the zero-width and
// direction marks that make a line read as something it isn't.
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u200b-\u200f\u2028-\u202e\u2060-\u2069\ufeff]/g;

export function forTerminal(text: string): string {
  return text.replace(SEQUENCE, '').replace(CONTROL, '');
}
