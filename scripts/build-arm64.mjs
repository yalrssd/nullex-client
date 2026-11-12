import { execSync } from 'node:child_process'

console.log('Building Nullex Client for Windows ARM64...')
execSync('pnpm tauri build --target aarch64-pc-windows-msvc', {
  stdio: 'inherit',
  shell: true,
})
console.log('Windows ARM64 build finished successfully.')
