// Shared flag parsing for the Validation Harness CLIs.
// `--flag value` sets flag=value; `--flag` (next token is another flag or
// absent) sets flag=true; positional tokens collect into `_`.

export function parseFlags(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (!a.startsWith('--')) {
      out._.push(a);
      continue;
    }
    const name = a.slice(2);
    const next = argv[i + 1];
    if (next === undefined || next.startsWith('--')) {
      out[name] = true;
    } else {
      out[name] = next;
      i += 1;
    }
  }
  return out;
}

export function required(args, names) {
  for (const name of names) {
    if (args[name] === undefined || args[name] === true) {
      throw new Error(`missing --${name}`);
    }
  }
}
