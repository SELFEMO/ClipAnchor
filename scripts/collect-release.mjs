import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const bundleRoot = path.join(root, 'src-tauri', 'target', 'release', 'bundle');
const releaseDir = path.join(root, 'release');
const platformMap = { win32: 'windows', darwin: 'macos', linux: 'linux' };
const archMap = { x64: 'x64', arm64: 'arm64', ia32: 'x86' };
const platform = platformMap[process.platform] || process.platform;
const arch = archMap[process.arch] || process.arch;
const wanted = new Set(['.exe', '.msi', '.dmg', '.deb', '.rpm']);

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
  // 中文：WiX 会为每种语言生成独立 MSI，因此复制到 release 时保留语言后缀，避免中英文安装包互相覆盖。
  // English: WiX emits one MSI per language, so we preserve the language suffix in release to avoid overwriting localized installers.
  const suffix = ext.toLowerCase() === '.msi' ? languageSuffix(artifact) : '';
  const target = path.join(releaseDir, `ClipAnchor_${platform}_${arch}${suffix}${ext}`);
  fs.copyFileSync(artifact, target);
  console.log(`Copied ${path.relative(root, artifact)} -> ${path.relative(root, target)}`);
}
