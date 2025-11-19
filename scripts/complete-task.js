#!/usr/bin/env node

import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const DOCS_DIR = path.join(__dirname, "..", "docs");
const TODO_DIR = path.join(DOCS_DIR, "tasks-todo");
const DONE_DIR = path.join(DOCS_DIR, "tasks-done");

function formatDate(date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function getLastModifiedDate(filePath) {
  const stats = fs.statSync(filePath);
  return new Date(stats.mtime);
}

function stripTaskNumber(filename) {
  let cleaned = filename.replace(/^task-[0-9x]+-/, "");
  cleaned = cleaned.replace(/^task-/, "");
  return cleaned;
}

function addDatePrefix(filename, date) {
  const dateStr = formatDate(date);
  const nameWithoutTaskPrefix = stripTaskNumber(filename);
  return `task-${dateStr}-${nameWithoutTaskPrefix}`;
}

function renameExistingTasks() {
  console.log("Renaming existing completed tasks...\n");

  const files = fs.readdirSync(DONE_DIR);
  const taskFiles = files.filter((f) => f.endsWith(".md"));

  let renamedCount = 0;
  let skippedCount = 0;

  taskFiles.forEach((filename) => {
    const oldPath = path.join(DONE_DIR, filename);

    if (/^task-\d{4}-\d{2}-\d{2}-/.test(filename)) {
      console.log(`Skipping (already dated): ${filename}`);
      skippedCount++;
      return;
    }

    const modifiedDate = getLastModifiedDate(oldPath);
    const newFilename = addDatePrefix(filename, modifiedDate);
    const newPath = path.join(DONE_DIR, newFilename);

    fs.renameSync(oldPath, newPath);
    console.log(`${filename} -> ${newFilename}`);
    renamedCount++;
  });

  console.log(`\nSummary:`);
  console.log(`  Renamed: ${renamedCount}`);
  console.log(`  Skipped: ${skippedCount}`);
  console.log(`  Total:   ${taskFiles.length}`);
}

function completeTask(taskIdentifier) {
  console.log(`Completing task: ${taskIdentifier}\n`);

  const todoFiles = fs.readdirSync(TODO_DIR);
  const matchingFile = todoFiles.find((f) => {
    const normalized = f.toLowerCase().replace(".md", "");
    const searchTerm = taskIdentifier.toLowerCase();
    return normalized.includes(searchTerm) || normalized.endsWith(searchTerm);
  });

  if (!matchingFile) {
    console.error(`Error: No task found matching "${taskIdentifier}"`);
    console.error(`\nAvailable tasks in tasks-todo/:`);
    todoFiles
      .filter((f) => f.endsWith(".md"))
      .forEach((f) => console.error(`   - ${f}`));
    process.exit(1);
  }

  const oldPath = path.join(TODO_DIR, matchingFile);
  const todayDate = new Date();
  const newFilename = addDatePrefix(matchingFile, todayDate);
  const newPath = path.join(DONE_DIR, newFilename);

  if (fs.existsSync(newPath)) {
    console.error(`Error: Task already exists in tasks-done: ${newFilename}`);
    process.exit(1);
  }

  fs.renameSync(oldPath, newPath);

  console.log(`Task completed!`);
  console.log(`   From: tasks-todo/${matchingFile}`);
  console.log(`   To:   tasks-done/${newFilename}`);
  console.log(`   Date: ${formatDate(todayDate)}`);
}

function showHelp() {
  console.log(`
Task Completion Script

Usage:
  bun task:complete TASK_NAME       Complete a task
  bun task:rename-done              Rename all existing completed tasks

Examples:
  bun task:complete frontend-performance
  bun task:complete 2
  bun task:rename-done

Notes:
  - Task name can be partial
  - Completed tasks are moved to tasks-done/ with format: task-YYYY-MM-DD-description.md
  - Existing tasks are renamed using their last modified date
`);
}

const args = process.argv.slice(2);

if (args.includes("--help") || args.includes("-h")) {
  showHelp();
  process.exit(0);
}

if (args.includes("--rename-existing")) {
  renameExistingTasks();
} else if (args.length === 0) {
  console.error("Error: No task specified\n");
  showHelp();
  process.exit(1);
} else {
  const taskIdentifier = args.join(" ");
  completeTask(taskIdentifier);
}
