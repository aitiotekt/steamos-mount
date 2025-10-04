import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

interface FileConfig {
    path: string;
    type: 'json' | 'toml';
}

const files: FileConfig[] = [
    { path: 'package.json', type: 'json' },
    { path: 'packages/core/Cargo.toml', type: 'toml' },
    { path: 'apps/cli/Cargo.toml', type: 'toml' },
    { path: 'apps/tui-app/Cargo.toml', type: 'toml' },
    { path: 'apps/tauri-app/package.json', type: 'json' },
    { path: 'apps/tauri-app/src-tauri/Cargo.toml', type: 'toml' },
    { path: 'apps/tauri-app/src-tauri/tauri.conf.json', type: 'json' },
    { path: 'apps/decky-plugin/package.json', type: 'json' },
    { path: 'apps/decky-plugin/pyproject.toml', type: 'toml' },
];

const newVersion = process.argv[2];

if (!newVersion) {
    console.error('Usage: node scripts/version-manage.ts <new_version>');
    process.exit(1);
}

// Basic semantic version validation
if (!/^\d+\.\d+\.\d+/.test(newVersion)) {
    console.error(`Invalid version format: ${newVersion}. Expected x.y.z`);
    process.exit(1);
}

console.log(`Bumping version to ${newVersion}...`);

let errors = 0;

for (const file of files) {
    try {
        const filePath = path.resolve(process.cwd(), file.path);
        if (!fs.existsSync(filePath)) {
            console.warn(`Warning: File not found: ${file.path}`);
            continue;
        }

        const content = fs.readFileSync(filePath, 'utf-8');
        let updatedContent = content;
        let replaced = false;

        if (file.type === 'json') {
            // For JSON, look for "version": "..."
            // We use regex to preserve formatting
            const regex = /"version":\s*"[^"]*"/;
            if (regex.test(content)) {
                updatedContent = content.replace(regex, `"version": "${newVersion}"`);
                replaced = true;
            }
        } else if (file.type === 'toml') {
            // For TOML, look for version = "..." at the start of line (for package definition)
            // This targets the package version which is usually at top level or start of line
            const regex = /^version\s*=\s*"[^"]*"/m;
            if (regex.test(content)) {
                updatedContent = content.replace(regex, `version = "${newVersion}"`);
                replaced = true;
            }
        }

        if (replaced) {
            if (content !== updatedContent) {
                fs.writeFileSync(filePath, updatedContent, 'utf-8');
                console.log(`Updated ${file.path}`);
            } else {
                console.log(`No change needed for ${file.path} (already ${newVersion})`);
            }
        } else {
            console.warn(`Warning: Could not find version pattern in ${file.path}`);
            errors++;
        }

    } catch (e) {
        console.error(`Error updating ${file.path}:`, e);
        errors++;
    }
}

if (errors > 0) {
    console.error(`Finished with ${errors} errors.`);
    process.exit(1);
} else {
    console.log('Successfully updated all files.');
}
