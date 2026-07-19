import { createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, parse } from 'node:path';

const require = createRequire(import.meta.url);

function findPackageJson(packageName) {
  try {
    return require.resolve(`${packageName}/package.json`);
  } catch (packageJsonError) {
    try {
      let directory = dirname(require.resolve(packageName));
      const root = parse(directory).root;
      while (directory !== root) {
        const candidate = join(directory, 'package.json');
        if (existsSync(candidate)) {
          const metadata = JSON.parse(readFileSync(candidate, 'utf8'));
          if (metadata.name === packageName) return candidate;
        }
        directory = dirname(directory);
      }
    } catch {
      // 下面统一抛出包含原始原因的错误，是为了让缺少依赖时只出现一条可操作的信息，而不是暴露多层模块解析堆栈。
      // The single actionable error below prevents a missing dependency from producing several nested module-resolution stacks.
    }
    throw new Error(`Missing local dependency ${packageName}. Run npm install before this command. (${packageJsonError.message})`);
  }
}

export function resolveNodeBinary(packageName, binaryName) {
  const packageJson = findPackageJson(packageName);
  const metadata = JSON.parse(readFileSync(packageJson, 'utf8'));
  const relativeEntry = typeof metadata.bin === 'string' ? metadata.bin : metadata.bin?.[binaryName];
  if (!relativeEntry) {
    throw new Error(`Package ${packageName} does not provide the ${binaryName} command.`);
  }

  const executable = join(dirname(packageJson), relativeEntry);
  if (!existsSync(executable)) {
    // 从包自身的 bin 元数据解析入口可兼容依赖升级后的文件名变化，也避免嵌套 npm 把 Tauri 参数误判为 npm 配置并输出警告。
    // Resolving the entry from the package bin metadata survives filename changes and avoids nested npm parsing Tauri arguments as npm configuration warnings.
    throw new Error(`The ${binaryName} command from ${packageName} is incomplete. Reinstall project dependencies.`);
  }
  return executable;
}
