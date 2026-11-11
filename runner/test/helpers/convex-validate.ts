// Checks a value against a Convex validator (convex/values' `v`), the way a
// deployment does: a function's args and return value, and every row written
// against its table in convex/schema.ts. convex-memory.ts runs the backend's
// functions with it, so a field one side writes and the schema or a validator
// doesn't have fails in the test, not on Mike's backend. The decision card's
// answer notes got through every test on 8 Nov because the fake took anything
// (the schema's asks had no optionNotes, so Convex would have refused them).
//
// convex/values 1.49 exports no validate function, so this walks the
// validator's JSON form: { kind, isOptional, fields | element | members |
// value | key | tableName }. Ids are strings, checked against their table only
// where the fake's own ids say it (`<table>:<n>`); storage ids are free text
// in tests.

type Validator = {
  kind: string;
  isOptional?: 'optional' | 'required';
  fields?: Record<string, Validator>;
  element?: Validator;
  members?: Validator[];
  value?: unknown;
  key?: Validator;
  tableName?: string;
};

/** The first way `value` breaks `validator`, as "at <path>: <why>", or null when it fits. */
export function problem(validator: Validator, value: unknown, at = ''): string | null {
  const here = at || 'the value';
  const kind = validator.kind;
  switch (kind) {
    case 'any':
      return value === undefined ? `${here}: missing` : null;
    case 'string':
      return typeof value === 'string' ? null : `${here}: wants a string, has ${show(value)}`;
    case 'float64':
      return typeof value === 'number' ? null : `${here}: wants a number, has ${show(value)}`;
    case 'int64':
      return typeof value === 'bigint' ? null : `${here}: wants an int64, has ${show(value)}`;
    case 'boolean':
      return typeof value === 'boolean' ? null : `${here}: wants a boolean, has ${show(value)}`;
    case 'null':
      return value === null ? null : `${here}: wants null, has ${show(value)}`;
    case 'bytes':
      return value instanceof ArrayBuffer ? null : `${here}: wants bytes, has ${show(value)}`;
    case 'literal':
      return value === validator.value ? null : `${here}: wants ${JSON.stringify(validator.value)}, has ${show(value)}`;
    case 'id': {
      if (typeof value !== 'string') return `${here}: wants an id of ${validator.tableName}, has ${show(value)}`;
      const table = /^(\w+):\d+$/.exec(value)?.[1];
      return table && validator.tableName !== '_storage' && table !== validator.tableName ? `${here}: wants an id of ${validator.tableName}, has one of ${table}` : null;
    }
    case 'array': {
      if (!Array.isArray(value)) return `${here}: wants an array, has ${show(value)}`;
      for (let i = 0; i < value.length; i++) {
        const p = problem(validator.element!, value[i], `${at}[${i}]`);
        if (p) return p;
      }
      return null;
    }
    case 'record': {
      if (!isPlain(value)) return `${here}: wants an object, has ${show(value)}`;
      for (const [k, x] of Object.entries(value)) {
        const p = problem(validator.key!, k, `${at}{key ${k}}`) ?? problem(validator.value as Validator, x, join(at, k));
        if (p) return p;
      }
      return null;
    }
    case 'object': {
      if (!isPlain(value)) return `${here}: wants an object, has ${show(value)}`;
      return objectProblem(validator.fields!, value, at);
    }
    case 'union': {
      const each = validator.members!.map((m) => problem(m, value, at));
      if (each.some((p) => p === null)) return null;
      // The member that got furthest says the most: the longest path.
      return each.sort((a, b) => b!.length - a!.length)[0] ?? `${here}: fits no member`;
    }
    default:
      return null;
  }
}

/** An object against a table's or a function's fields: every field it has is known, every required one is there. */
export function objectProblem(fields: Record<string, Validator>, value: Record<string, unknown>, at = ''): string | null {
  for (const [k, x] of Object.entries(value)) {
    if (x === undefined) continue;
    const f = fields[k];
    if (!f) return `${join(at, k)}: not in the validator (Convex refuses a field it doesn't know)`;
    const p = problem(f, x, join(at, k));
    if (p) return p;
  }
  for (const [k, f] of Object.entries(fields)) {
    if (f.isOptional !== 'optional' && value[k] === undefined) return `${join(at, k)}: missing`;
  }
  return null;
}

function join(at: string, k: string): string {
  return at ? `${at}.${k}` : k;
}

function isPlain(x: unknown): x is Record<string, unknown> {
  return typeof x === 'object' && x !== null && !Array.isArray(x) && !(x instanceof ArrayBuffer);
}

function show(x: unknown): string {
  if (x === undefined) return 'nothing';
  if (typeof x === 'bigint') return `${x}n`;
  const s = JSON.stringify(x);
  return s && s.length > 60 ? `${s.slice(0, 57)}...` : String(s);
}
