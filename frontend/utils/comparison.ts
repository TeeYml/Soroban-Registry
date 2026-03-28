import type { Contract } from '@/lib/api';

export type ComparisonMetricKey =
  | 'abi_method_count'
  | 'gas_estimate'
  | 'deployment_count'
  | 'verification_status';

export type CellTone = 'neutral' | 'best' | 'worst';

export type DiffLine =
  | { type: 'context'; value: string }
  | { type: 'add'; value: string }
  | { type: 'remove'; value: string };

export interface ComparableContract {
  id: string;
  name: string;
  abiMethods: string[];
  gasEstimate: number;
  deploymentCount: number;
  isVerified: boolean;
  sourceCode: string;
  base?: Pick<Contract, 'contract_id' | 'network' | 'publisher_id' | 'wasm_hash' | 'updated_at'>;
}

function hashStringToUint32(input: string) {
  let h = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function seededInt(seed: number, min: number, max: number) {
  const x = Math.imul(seed ^ 0x9e3779b9, 0x85ebca6b) >>> 0;
  const n = x / 0xffffffff;
  return Math.floor(min + n * (max - min + 1));
}

function seededChoice<T>(seed: number, items: T[]) {
  if (items.length === 0) {
    throw new Error('seededChoice: empty items');
  }
  return items[seededInt(seed, 0, items.length - 1)];
}

function buildMockAbiMethods(seed: number) {
  const verbs = ['get', 'set', 'mint', 'burn', 'transfer', 'swap', 'quote', 'deposit', 'withdraw', 'claim'];
  const nouns = ['balance', 'allowance', 'owner', 'admin', 'price', 'pool', 'token', 'reward', 'position', 'config'];
  const count = seededInt(seed ^ 0x1234, 6, 22);
  const methods = new Set<string>();
  for (let i = 0; i < count; i += 1) {
    const verb = seededChoice(seed + i * 31, verbs);
    const noun = seededChoice(seed + i * 97, nouns);
    const suffix = seededInt(seed + i * 13, 0, 3);
    const name = suffix === 0 ? `${verb}_${noun}` : `${verb}_${noun}_${suffix}`;
    methods.add(name);
  }
  return [...methods].sort((a, b) => a.localeCompare(b));
}

function buildMockSourceCode(seed: number, name: string, isVerified: boolean) {
  const header = `// ${name}\n// Generated preview source (mock)\n`;
  const verifiedLine = isVerified ? 'const VERIFIED: bool = true;\n' : 'const VERIFIED: bool = false;\n';
  const functions = buildMockAbiMethods(seed)
    .slice(0, 10)
    .map((m) => `pub fn ${m}(env: Env) -> i128 {\n    env.ledger().sequence() as i128\n}\n`)
    .join('\n');
  return `${header}\nuse soroban_sdk::{contract, contractimpl, Env};\n\n${verifiedLine}\n#[contract]\npub struct Contract;\n\n#[contractimpl]\nimpl Contract {\n${indentBlock(functions.trimEnd(), 2)}\n}\n`;
}

function indentBlock(text: string, spaces: number) {
  const pad = ' '.repeat(spaces);
  return text
    .split('\n')
    .map((l) => (l.length > 0 ? `${pad}${l}` : l))
    .join('\n');
}

export function toComparableContract(contract: Contract): ComparableContract {
  const seed = hashStringToUint32(`${contract.id}:${contract.contract_id}:${contract.wasm_hash}`);
  const isVerified = Boolean(contract.is_verified);
  const abiMethods = buildMockAbiMethods(seed);
  const gasEstimate = seededInt(seed ^ 0x55aa, 12000, 95000);
  const deploymentCount = seededInt(seed ^ 0xaa55, 1, 250);
  const sourceCode = buildMockSourceCode(seed, contract.name, isVerified);

  return {
    id: contract.id,
    name: contract.name,
    abiMethods,
    gasEstimate,
    deploymentCount,
    isVerified,
    sourceCode,
    base: {
      contract_id: contract.contract_id,
      network: contract.network,
      publisher_id: contract.publisher_id,
      wasm_hash: contract.wasm_hash,
      updated_at: contract.updated_at,
    },
  };
}

export function toneForMetricCell(
  metric: ComparisonMetricKey,
  value: number | boolean,
  allValues: Array<number | boolean>,
): CellTone {
  const isAllEqual =
    allValues.length > 0 && allValues.every((v) => v === allValues[0]);
  if (isAllEqual) return 'neutral';

  if (metric === 'verification_status') {
    const v = Boolean(value);
    return v ? 'best' : 'worst';
  }

  const nums = allValues.map((v) => Number(v));
  const n = Number(value);
  const min = Math.min(...nums);
  const max = Math.max(...nums);

  const higherIsBetter = metric === 'deployment_count' || metric === 'abi_method_count';
  const best = higherIsBetter ? max : min;
  const worst = higherIsBetter ? min : max;

  if (n === best && n !== worst) return 'best';
  if (n === worst && n !== best) return 'worst';
  return 'neutral';
}

export function getMetricValue(contract: ComparableContract, metric: ComparisonMetricKey): number | boolean {
  switch (metric) {
    case 'abi_method_count':
      return contract.abiMethods.length;
    case 'gas_estimate':
      return contract.gasEstimate;
    case 'deployment_count':
      return contract.deploymentCount;
    case 'verification_status':
      return contract.isVerified;
  }
}

export function diffMethodSets(base: string[], other: string[]) {
  const baseSet = new Set(base);
  const otherSet = new Set(other);
  const added = [...otherSet].filter((m) => !baseSet.has(m)).sort((a, b) => a.localeCompare(b));
  const removed = [...baseSet].filter((m) => !otherSet.has(m)).sort((a, b) => a.localeCompare(b));
  return { added, removed };
}

export function diffLines(aText: string, bText: string): DiffLine[] {
  const a = normalizeNewlines(aText).split('\n');
  const b = normalizeNewlines(bText).split('\n');
  const result = myersDiff(a, b);
  return result;
}

function normalizeNewlines(s: string) {
  return s.replaceAll('\r\n', '\n').replaceAll('\r', '\n');
}

function myersDiff(a: string[], b: string[]): DiffLine[] {
  const n = a.length;
  const m = b.length;
  const max = n + m;
  const v = new Map<number, number>();
  v.set(1, 0);
  const trace: Array<Map<number, number>> = [];

  for (let d = 0; d <= max; d += 1) {
    const snapshot = new Map<number, number>();
    for (let k = -d; k <= d; k += 2) {
      const down = k === -d;
      const up = k === d;
      const kPrev = down ? k + 1 : up ? k - 1 : (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0) ? k + 1 : k - 1;
      const xStart = v.get(kPrev) ?? 0;
      const x = down || (!up && (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0)) ? xStart : xStart + 1;
      let y = x - k;
      let xWalk = x;
      while (xWalk < n && y < m && a[xWalk] === b[y]) {
        xWalk += 1;
        y += 1;
      }
      snapshot.set(k, xWalk);
      if (xWalk >= n && y >= m) {
        trace.push(snapshot);
        return backtrackDiff(a, b, trace);
      }
    }
    trace.push(snapshot);
    v.clear();
    for (const [k, xVal] of snapshot.entries()) v.set(k, xVal);
  }

  return backtrackDiff(a, b, trace);
}

function backtrackDiff(a: string[], b: string[], trace: Array<Map<number, number>>): DiffLine[] {
  let x = a.length;
  let y = b.length;
  const edits: DiffLine[] = [];

  for (let d = trace.length - 1; d >= 0; d -= 1) {
    const v = trace[d];
    const k = x - y;

    const down = k === -d;
    const up = k === d;

    const kPrev = down ? k + 1 : up ? k - 1 : (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0) ? k + 1 : k - 1;
    const xPrev = v.get(kPrev) ?? 0;
    const yPrev = xPrev - kPrev;

    while (x > xPrev && y > yPrev) {
      edits.push({ type: 'context', value: a[x - 1] ?? '' });
      x -= 1;
      y -= 1;
    }

    if (d === 0) break;

    if (x === xPrev) {
      edits.push({ type: 'add', value: b[y - 1] ?? '' });
      y -= 1;
    } else {
      edits.push({ type: 'remove', value: a[x - 1] ?? '' });
      x -= 1;
    }
  }

  edits.reverse();
  return coalesceContext(edits);
}

function coalesceContext(lines: DiffLine[]) {
  const out: DiffLine[] = [];
  for (const line of lines) {
    if (out.length === 0) {
      out.push(line);
      continue;
    }
    const prev = out[out.length - 1];
    if (prev.type === 'context' && line.type === 'context') {
      out.push(line);
      continue;
    }
    out.push(line);
  }
  return out;
}

