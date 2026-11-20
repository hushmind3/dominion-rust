//! The single native agent harness. It orchestrates model turns, durable
//! Run transitions and tool intentions through the Engine and runtime ports.
//! Agent strategies in src/algorithms/ only *decide*; no second Run loop.
mod runner;
