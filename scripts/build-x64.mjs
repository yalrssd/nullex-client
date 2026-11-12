import { execSync } from 'node:child_process'

console.log('Building Nullex Client for Windows x64...')
execSync('pnpm tauri build --target x86_64-pc-windows-msvc', {
  stdio: 'inherit',
  shell: true,
})
console.log('Windows x64 build finished successfully.')
