import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import process from 'node:process';

const archMap = {
  x64: 'x64',
  arm64: 'arm64',
};

const ridMap = {
  win32: {
    x64: 'win-x64',
    arm64: 'win-arm64',
  },
  darwin: {
    x64: 'osx-x64',
    arm64: 'osx-arm64',
  },
  linux: {
    x64: 'linux-x64',
    arm64: 'linux-arm64',
  },
};

const platform = process.platform;
const arch = archMap[process.arch];
if (!arch || !ridMap[platform]?.[arch]) {
  console.error(`Unsupported platform/arch: ${platform}/${process.arch}`);
  process.exit(1);
}

const rid = process.env.FINDAMODEL_RID ?? ridMap[platform][arch];
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const backendProject = path.join(repoRoot, 'backend', 'findamodel.csproj');
const publishDir = path.join(repoRoot, 'backend', 'artifacts', 'desktop', rid);
const tauriBinDir = path.join(repoRoot, 'desktop-tauri', 'src-tauri', 'bin');

mkdirSync(tauriBinDir, { recursive: true });

const publishArgs = [
  'publish',
  backendProject,
  '-c',
  'Release',
  '-r',
  rid,
  '--self-contained',
  'true',
  '-o',
  publishDir,
  '/p:PublishSingleFile=true',
  '/p:PublishTrimmed=false',
  '/p:IncludeNativeLibrariesForSelfExtract=true',
];

const publish = spawnSync('dotnet', publishArgs, {
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

if (publish.status !== 0) {
  process.exit(publish.status ?? 1);
}

const sourceExe = path.join(publishDir, process.platform === 'win32' ? 'findamodel.exe' : 'findamodel');
const targetExe = path.join(
  tauriBinDir,
  process.platform === 'win32' ? 'findamodel-backend.exe' : 'findamodel-backend',
);

if (!existsSync(sourceExe)) {
  console.error(`Published backend executable not found at ${sourceExe}`);
  process.exit(1);
}

copyFileSync(sourceExe, targetExe);
console.log(`Published sidecar copied to ${targetExe}`);
