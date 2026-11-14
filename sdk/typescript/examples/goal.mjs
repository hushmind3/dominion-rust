// Run after npm install && npm run build, with the Dominion Rust server running.
// Usage: DOMINION_ADMIN_TOKEN=... node examples/goal.mjs "Implement a small project and test it"
import {DominionClient} from "../dist/index.js";

const goalText = process.argv.slice(2).join(" ").trim();
if (!goalText) {
  console.error('Usage: node examples/goal.mjs "Your autonomous goal"');
  process.exitCode = 2;
} else {
  const dominion = new DominionClient({
    baseUrl: process.env.DOMINION_URL ?? "http://127.0.0.1:4317",
    adminToken: process.env.DOMINION_ADMIN_TOKEN,
  });
  const goal = await dominion.goals.create({
    goal: goalText,
    maxIterations: 16,
    isolateWorktree: false,
  });
  console.log("Goal:", goal.id, "job:", goal.jobId);
  const final = await dominion.goals.wait(goal.id, {
    timeoutMs: 600_000,
    onUpdate: current => console.log("Phase:", current.phase, "status:", current.status),
  });
  console.log("Final state:", final.status);
  if (final.status === "needs_review") {
    console.log("Review required:", final.gaps);
    console.log("Do not blindly retry a possibly side-effecting operation.");
    process.exitCode = 2;
  } else if (final.status !== "completed") {
    process.exitCode = 1;
  }
}
