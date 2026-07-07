import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const releaseDir = path.join(root, 'release');
const platformMap = { win32: 'Windows', darwin: 'macOS', linux: 'Linux' };
const archMap = { x64: 'x64', arm64: 'arm64', ia32: 'x86' };
const targetTriple = readTargetTriple();
const platform = targetTriple ? platformFromTriple(targetTriple) : (platformMap[process.platform] || process.platform);
const arch = targetTriple ? archFromTriple(targetTriple) : (archMap[process.arch] || process.arch);
const bundleRoot = targetTriple
  ? path.join(root, 'src-tauri', 'target', targetTriple, 'release', 'bundle')
  : path.join(root, 'src-tauri', 'target', 'release', 'bundle');
const wanted = new Set(['.exe', '.msi', '.dmg', '.deb', '.rpm']);

function readTargetTriple() {
  const targetIndex = process.argv.indexOf('--target');
  if (targetIndex >= 0 && process.argv[targetIndex + 1]) return process.argv[targetIndex + 1];
  return process.env.TAURI_TARGET_TRIPLE || process.env.CARGO_BUILD_TARGET || '';
}

function platformFromTriple(triple) {
  const value = triple.toLowerCase();
  if (value.includes('apple-darwin')) return 'macOS';
  if (value.includes('windows')) return 'Windows';
  if (value.includes('linux')) return 'Linux';
  return platformMap[process.platform] || process.platform;
}

function archFromTriple(triple) {
  const value = triple.toLowerCase();
  if (value.startsWith('aarch64') || value.startsWith('arm64')) return 'arm64';
  if (value.startsWith('x86_64') || value.startsWith('amd64')) return 'x64';
  if (value.startsWith('i686') || value.startsWith('i386')) return 'x86';
  return archMap[process.arch] || process.arch;
}

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  const entries = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) entries.push(...walk(full));
    else entries.push(full);
  }
  return entries;
}

function languageSuffix(file) {
  const name = path.basename(file).toLowerCase();
  const knownLanguages = [
    ['en-us', 'en-US'],
    ['zh-cn', 'zh-CN'],
    ['zh_hans', 'zh-CN'],
    ['simpchinese', 'zh-CN'],
    ['english', 'en-US']
  ];

  for (const [needle, suffix] of knownLanguages) {
    if (name.includes(needle)) return `_${suffix}`;
  }

  return '';
}

fs.mkdirSync(releaseDir, { recursive: true });
const artifacts = walk(bundleRoot).filter((file) => wanted.has(path.extname(file).toLowerCase()));
if (!artifacts.length) {
  console.warn(`No installer artifacts found under ${bundleRoot}`);
  process.exit(0);
}

for (const artifact of artifacts) {
  const ext = path.extname(artifact);
  // 中文：交叉构建会把产物写入 target/<triple>/release/bundle，因此命名必须优先使用目标 triple 而不是当前机器架构。
  // English: Cross builds write artifacts under target/<triple>/release/bundle, so naming must prefer the target triple rather than the host architecture.
  const suffix = ext.toLowerCase() === '.msi' ? languageSuffix(artifact) : '';
  const target = path.join(releaseDir, `ClipAnchor_${platform}_${arch}${suffix}${ext}`);
  fs.copyFileSync(artifact, target);
  console.log(`Copied ${path.relative(root, artifact)} -> ${path.relative(root, target)}`);
}
