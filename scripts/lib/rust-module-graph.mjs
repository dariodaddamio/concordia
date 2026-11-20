import { existsSync, readFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

export const PARSER_REVISION = 2;
// Static source references and use trees, without compiler name/feature/macro resolution.
// Preserve offsets so cfg attributes can remove exactly their item, including early test helpers.
export function productionRust(source) {
  const text = source.split("");
  const blank = (start, end) => {
    for (let i = start; i < end; i++) if (text[i] !== "\n") text[i] = " ";
  };
  for (let i = 0; i < source.length;) {
    const start = i;
    if (source.startsWith("//", i)) {
      i = source.indexOf("\n", i);
      if (i < 0) i = source.length;
    } else if (source.startsWith("/*", i)) {
      let depth = 1;
      i += 2;
      while (i < source.length && depth) {
        if (source.startsWith("/*", i)) { depth++; i += 2; }
        else if (source.startsWith("*/", i)) { depth--; i += 2; }
        else i++;
      }
    } else {
      const raw = /^(?:b|c)?r(#{0,255})"/u.exec(source.slice(i));
      if (raw) {
        const close = `"${raw[1]}`;
        const end = source.indexOf(close, i + raw[0].length);
        i = end < 0 ? source.length : end + close.length;
      } else if (source[i] === '"') {
        i++;
        while (i < source.length) {
          if (source[i] === "\\") i += 2;
          else if (source[i++] === '"') break;
        }
      } else if (source[i] === "'" && /^'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'/u.test(source.slice(i))) {
        i += /^'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'/u.exec(source.slice(i))[0].length;
      } else { i++; continue; }
    }
    blank(start, i);
  }
  let code = text.join("");
  const tests = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/gu;
  for (let match; (match = tests.exec(code));) {
    let i = tests.lastIndex;
    // Attributes on the item can contain braces and semicolons in erased literals.
    while (/\s/u.test(code[i] ?? "")) i++;
    while (code.startsWith("#[", i)) {
      const end = code.indexOf("]", i + 2);
      if (end < 0) throw new Error("Unterminated Rust attribute");
      i = end + 1;
      while (/\s/u.test(code[i] ?? "")) i++;
    }
    while (i < code.length && code[i] !== "{" && code[i] !== ";") i++;
    if (code[i] === "{") {
      let depth = 1;
      while (++i < code.length && depth) {
        if (code[i] === "{") depth++;
        else if (code[i] === "}") depth--;
      }
    } else if (code[i] === ";") i++;
    blank(match.index, i);
    code = text.join("");
    tests.lastIndex = i;
  }
  return code;
}

export function moduleGraph(sourceRoot) {
  const modules = new Map();
  const visit = (name, file, inlineSource, moduleDirectory) => {
    file = resolve(file);
    if (modules.has(name)) return;
    const source = inlineSource ?? readFileSync(file, "utf8");
    let code = productionRust(source);
    const directory = moduleDirectory ?? (["mod.rs", "lib.rs"].includes(basename(file))
      ? dirname(file) : join(dirname(file), basename(file, ".rs")));
    modules.set(name, { file, code });
    const declarations = /\bmod\s+([A-Za-z_]\w*)\s*([;{])/gu;
    for (let match; (match = declarations.exec(code));) {
      const childName = name ? `${name}::${match[1]}` : match[1];
      if (match[2] === "{") {
        const start = declarations.lastIndex;
        let depth = 1, end = start;
        while (end < code.length && depth) {
          if (code[end] === "{") depth++;
          else if (code[end] === "}") depth--;
          end++;
        }
        if (depth) throw new Error(`Unterminated inline module ${childName}`);
        visit(childName, file, source.slice(start, end - 1), join(directory, match[1]));
        code = code.slice(0, match.index) + code.slice(match.index, end).replace(/[^\n]/gu, " ") + code.slice(end);
        declarations.lastIndex = end;
        continue;
      }
      const path = /#\[path\s*=\s*"([^"]+)"\]\s*(?:pub(?:\([^)]*\))?\s*)?$/u.exec(source.slice(Math.max(0, match.index - 512), match.index));
      let child = path ? join(dirname(file), path[1]) : join(directory, `${match[1]}.rs`);
      if (!path && !existsSync(child)) child = join(directory, match[1], "mod.rs");
      if (!existsSync(child)) throw new Error(`Cannot resolve Rust module ${name}::${match[1]} from ${file}`);
      visit(childName, child);
    }
    modules.set(name, { file, code });
  };
  visit("", join(sourceRoot, "lib.rs"));
  const edges = new Map();
  const normalize = (name, parts) => {
    const current = name ? name.split("::") : [];
    if (parts[0] === "crate") return parts.slice(1);
    if (parts[0] === "self") return [...current, ...parts.slice(1)];
    if (parts[0] === "super") {
      while (parts[0] === "super") { parts = parts.slice(1); current.pop(); }
      return [...current, ...parts];
    }
    if (modules.has([...current, parts[0]].join("::"))) return [...current, ...parts];
    return parts;
  };
  const moduleOf = (parts) => {
    while (parts.length && !modules.has(parts.join("::"))) parts.pop();
    return parts.join("::");
  };
  for (const [name, { code }] of modules) {
    if (!name) continue;
    const targets = new Set();
    const aliases = new Map();
    const add = (parts) => {
      const target = moduleOf(parts);
      if (target && target !== name) targets.add(target);
    };
    for (const match of code.matchAll(/\buse\s+([^;]+);/gu)) {
      const tokens = match[1].match(/[A-Za-z_]\w*|::|[{},*]/gu) ?? [];
      let index = 0;
      const tree = (prefix) => {
        let path = [...prefix];
        while (index < tokens.length) {
          const token = tokens[index++];
          if (token === "::") continue;
          if (token === "{") {
            while (index < tokens.length && tokens[index] !== "}") {
              tree(path);
              if (tokens[index] === ",") index++;
            }
            if (tokens[index] === "}") index++;
            return;
          }
          if (token === "," || token === "}") { index--; break; }
          if (token === "as") {
            const alias = tokens[index++];
            const absolute = normalize(name, path);
            aliases.set(alias, absolute); add([...absolute]); return;
          }
          if (token === "self" && path.length) break;
          if (token !== "*") path.push(token);
        }
        const absolute = normalize(name, path);
        aliases.set(path.at(-1), absolute); add([...absolute]);
      };
      tree([]);
    }
    for (const match of code.matchAll(/\b([A-Za-z_]\w*(?:::[A-Za-z_]\w*)+)/gu)) {
      const parts = match[1].split("::");
      const alias = aliases.get(parts[0]);
      add(alias ? [...alias, ...parts.slice(1)] : normalize(name, parts));
    }
    edges.set(name, targets);
  }
  return { modules, edges };
}

export function ownershipEdges(edges) {
  const owners = new Map();
  for (const [name, targets] of edges) {
    const owner = name.split("::")[0];
    if (!owners.has(owner)) owners.set(owner, new Set());
    for (const target of targets) {
      const other = target.split("::")[0];
      if (other !== owner) owners.get(owner).add(other);
    }
  }
  return owners;
}

export function cycleComponents(edges) {
  const numbers = new Map(), low = new Map(), stack = [], active = new Set(), components = [];
  let next = 0;
  const visit = (name) => {
    numbers.set(name, next); low.set(name, next++); stack.push(name); active.add(name);
    for (const target of edges.get(name) ?? []) {
      if (!numbers.has(target)) { visit(target); low.set(name, Math.min(low.get(name), low.get(target))); }
      else if (active.has(target)) low.set(name, Math.min(low.get(name), numbers.get(target)));
    }
    if (low.get(name) !== numbers.get(name)) return;
    const component = [];
    for (let node; (node = stack.pop()) !== undefined;) {
      active.delete(node); component.push(node);
      if (node === name) break;
    }
    if (component.length > 1) components.push(component.sort());
  };
  for (const name of edges.keys()) if (!numbers.has(name)) visit(name);
  return components.sort((a, b) => a[0].localeCompare(b[0]));
}

// Return the directed edges that participate in at least one cycle. Keeping this edge-level
// view prevents a large SCC from being mistaken for a single intentional architectural cycle.
export function cycleEdges(edges) {
  const reaches = (start, goal) => {
    const seen = new Set([start]);
    const queue = [start];
    while (queue.length) {
      const current = queue.shift();
      for (const next of edges.get(current) ?? []) {
        if (next === goal) return true;
        if (!seen.has(next)) { seen.add(next); queue.push(next); }
      }
    }
    return false;
  };
  return [...edges].flatMap(([from, targets]) =>
    [...targets].filter((to) => reaches(to, from)).map((to) => [from, to]));
}

if (process.argv[2] === "--check") {
  const sourceRoot = process.argv[3], baselineFile = process.argv[4];
  const baseline = readFileSync(baselineFile, "utf8");
  if (!baseline.includes(`parser revision ${PARSER_REVISION}.`)) throw new Error("Cycle baseline parser revision mismatch");
  const allowed = new Set([...baseline.matchAll(/^edge ([A-Za-z_]\w* -> [A-Za-z_]\w*): \S.+$/gmu)].map(match => match[1]));
  if (!allowed.size || /^\s*[A-Za-z_]\w*:/mu.test(baseline)) throw new Error("Cycle baseline must contain exact named edges, not module exemptions");
  const graph = moduleGraph(sourceRoot);
  const cyclic = cycleEdges(ownershipEdges(graph.edges)).map(edge => edge.join(" -> "));
  const violations = cyclic.filter(edge => !allowed.has(edge));
  if (violations.length) {
    process.stderr.write(`Unapproved static ownership cycle edges:\n${violations.join("\n")}\n`);
    process.exitCode = 1;
  } else console.log(`Static source-reference cycle gate passed: ${graph.modules.size} modules, ${cyclic.length} historical cyclic edges. Macro expansion, feature evaluation and compiler name resolution are outside this gate.`);
}
