// Sound utilities for UI notifications with singleton AudioContext pattern

import { SOUND } from '../constants';
import { getSoundMuted } from './storage';

/**
 * Singleton AudioContext manager
 * Reuses single context to avoid browser limits (max 6 contexts)
 */
class AudioManager {
  private context: AudioContext | null = null;
  private lastPlayTime: number = 0;
  private buffer: AudioBuffer | null = null;

  /**
   * Get or create AudioContext (lazy initialization)
   */
  private getContext(): AudioContext | null {
    if (this.context) {
      return this.context;
    }

    try {
      this.context = new (window.AudioContext || (window as any).webkitAudioContext)();

      // Handle suspended state (browser autoplay policy)
      if (this.context.state === 'suspended') {
        // Try to resume, but don't block if it fails
        this.context.resume().catch(() => {
          console.debug('[sound] AudioContext suspended, will resume on user interaction');
        });
      }

      // Pre-generate audio buffer for reuse
      this.buffer = this.createClickBuffer(this.context);

      return this.context;
    } catch (error) {
      console.debug('[sound] Failed to create AudioContext:', error);
      return null;
    }
  }

  /**
   * Create reusable audio buffer for click sound
   * Uses deterministic waveform (exponentially decaying sine wave)
   */
  private createClickBuffer(context: AudioContext): AudioBuffer {
    const duration = 0.05; // 50ms
    const sampleRate = context.sampleRate;
    const buffer = context.createBuffer(1, sampleRate * duration, sampleRate);
    const data = buffer.getChannelData(0);

    // Generate click: high-frequency sine wave with exponential decay
    const frequency = 3000; // 3kHz for sharp click
    for (let i = 0; i < buffer.length; i++) {
      const t = i / sampleRate;
      const envelope = Math.exp(-t * 120); // Fast decay
      const wave = Math.sin(2 * Math.PI * frequency * t);
      data[i] = wave * envelope * 0.3;
    }

    return buffer;
  }

  /**
   * Play trade click sound with throttling and mute check
   */
  public playTradeClick(): void {
    // Check mute preference
    if (getSoundMuted()) {
      return;
    }

    // Throttle: prevent sound spam on rapid trades
    const now = Date.now();
    if (now - this.lastPlayTime < SOUND.TRADE_CLICK_THROTTLE_MS) {
      return;
    }
    this.lastPlayTime = now;

    const context = this.getContext();
    if (!context || !this.buffer) {
      return;
    }

    try {
      // Resume context if suspended (browser autoplay policy)
      if (context.state === 'suspended') {
        context.resume();
      }

      // Create and connect nodes
      const source = context.createBufferSource();
      source.buffer = this.buffer;

      const gainNode = context.createGain();
      gainNode.gain.value = SOUND.TRADE_CLICK_VOLUME;

      source.connect(gainNode);
      gainNode.connect(context.destination);

      // Play the sound
      source.start(0);

      // Clean up after sound finishes (don't close context, reuse it)
      source.onended = () => {
        gainNode.disconnect();
        source.disconnect();
      };
    } catch (error) {
      console.debug('[sound] Failed to play trade click:', error);
    }
  }

  /**
   * Prime the shared AudioContext during a user gesture so future trade sounds
   * can play without requiring the first live trade itself to unlock audio.
   */
  public prime(): void {
    const context = this.getContext();
    if (!context) {
      return;
    }
    if (context.state === 'suspended') {
      context.resume().catch(() => {
        console.debug('[sound] Failed to resume AudioContext during prime');
      });
    }
  }

  /**
   * Clean up AudioContext (call on app unmount)
   */
  public cleanup(): void {
    if (this.context) {
      this.context.close().catch(() => {
        // Ignore errors on cleanup
      });
      this.context = null;
      this.buffer = null;
    }
  }
}

// Singleton instance
const audioManager = new AudioManager();

/**
 * Play a brief click sound for trade notifications
 * - Respects user mute preference
 * - Throttled to max 1 sound per 100ms
 * - Reuses singleton AudioContext for performance
 */
export function playTradeClick(): void {
  audioManager.playTradeClick();
}

export function primeTradeAudio(): void {
  audioManager.prime();
}

/**
 * Clean up audio resources (call on app unmount)
 */
export function cleanupAudio(): void {
  audioManager.cleanup();
}
