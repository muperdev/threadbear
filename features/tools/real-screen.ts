// For the tools that use the real screen and pointer, which run only on a
// machine nobody's using (Tinker Desk's desktop, AGENTS.md): where a window is
// on screen, and the Swift helpers in app/Tools that post real events.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
export const work = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-real-screen-'));
process.on('exit', () => fs.rmSync(work, { recursive: true, force: true }));

/** app/Tools/<name>.swift, compiled once into a temp folder. */
export function swiftTool(name: string): string {
  const out = path.join(work, name);
  if (!fs.existsSync(out)) execFileSync('swiftc', ['-O', path.join(root, 'app/Tools', `${name}.swift`), '-o', out]);
  return out;
}

const framer = path.join(work, 'frame');

/** The process's main window: x, y, width and height in screen points from the top left. */
export function windowFrame(pid: number): number[] {
  if (!fs.existsSync(framer)) {
    fs.writeFileSync(`${framer}.swift`, `import CoreGraphics
import Foundation
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
  let b = w[kCGWindowBounds as String] as! [String: Double]
  print(Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!))
  break
}
`);
    execFileSync('swiftc', ['-O', `${framer}.swift`, '-o', framer]);
  }
  return execFileSync(framer, [String(pid)]).toString().trim().split(' ').map(Number);
}

const finder = path.join(work, 'window-at');

/** The app whose window is on top at a screen point: what a real click there reaches. */
export function windowAt(x: number, y: number): { pid: number; owner: string } | null {
  if (!fs.existsSync(finder)) {
    fs.writeFileSync(`${finder}.swift`, `import CoreGraphics
import Foundation
let x = Double(CommandLine.arguments[1])!, y = Double(CommandLine.arguments[2])!
// Front to back, every layer: a notification or a dialog over the point takes the click too.
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list {
  guard let b = w[kCGWindowBounds as String] as? [String: Double], (w[kCGWindowAlpha as String] as? Double ?? 1) > 0 else { continue }
  // Notification Centre keeps a see-through window over the whole screen
  // that only its banners take clicks in.
  if (w[kCGWindowName as String] as? String) == "Notification Center" { continue }
  if x >= b["X"]! && x < b["X"]! + b["Width"]! && y >= b["Y"]! && y < b["Y"]! + b["Height"]! {
    print(w[kCGWindowOwnerPID as String] as! Int32, w[kCGWindowOwnerName as String] as? String ?? "?")
    break
  }
}
`);
    execFileSync('swiftc', ['-O', `${finder}.swift`, '-o', finder]);
  }
  const out = execFileSync(finder, [String(x), String(y)]).toString().trim();
  if (!out) return null;
  const [pid, ...owner] = out.split(' ');
  return { pid: Number(pid), owner: owner.join(' ') };
}

/**
 * Stops before any real event if the app's window isn't the one on top at
 * every point it's about to press: on 10 Nov another job's windows covered
 * the test window on Tinker Desk, and the drags went to them instead.
 */
export function assertInFront(pid: number, points: [number, number][]): void {
  for (const [x, y] of points) {
    const top = windowAt(x, y);
    if (top?.pid !== pid) {
      throw new Error(`Not in front at (${x}, ${y}): ${top ? top.owner : 'nothing'} is there. No events were posted.`);
    }
  }
}
