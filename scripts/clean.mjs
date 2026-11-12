import fs from 'node:fs'
import path from 'node:path'

const dirsToClean = [
  path.resolve('dist'),
  path.resolve('src-tauri/target/release/bundle'),
]

for (const dir of dirsToClean) {
  if (fs.existsSync(dir)) {
    console.log(`Cleaning ${dir}...`)
    fs.rmSync(dir, { recursive: true, force: true })
  }
}

console.log('Clean complete.')
