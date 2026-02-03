/**
 * Preload script: initializes the FFI library once before all test files.
 */
import { getLib } from "../src/lib";

// Force library initialization before any test file runs
getLib();
