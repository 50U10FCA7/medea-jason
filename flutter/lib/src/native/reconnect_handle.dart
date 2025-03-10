import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';

import '../interface/reconnect_handle.dart';
import '../util/move_semantic.dart';
import '../util/rust_opaque.dart';
import '/src/util/rust_handles_storage.dart';
import 'ffi/jason_api.g.dart' as frb;
import 'jason.dart';

class NativeReconnectHandle implements ReconnectHandle {
  /// `flutter_rust_bridge` Rust opaque type backing this object.
  final RustOpaque<frb.ReconnectHandle> opaque;

  /// Constructs a new [ReconnectHandle] backed by the Rust struct behind the
  /// provided [frb.ReconnectHandle].
  NativeReconnectHandle(frb.ReconnectHandle reconnectHandle)
      : opaque = RustOpaque(reconnectHandle) {
    RustHandlesStorage().insertHandle(this);
  }

  @override
  Future<void> reconnectWithDelay(int delayMs) async {
    if (delayMs.isNegative || delayMs.bitLength > 32) {
      throw ArgumentError.value(delayMs, 'delayMs', 'Expected `u32`');
    }

    try {
      await (api.reconnectHandleReconnectWithDelay(
          reconnectHandle: opaque.innerOpaque, delayMs: delayMs) as Future);
    } on FfiException catch (anyhow) {
      throw anyhow.parse();
    }
  }

  @override
  Future<void> reconnectWithBackoff(
      int startingDelayMs, double multiplier, int maxDelay,
      [int? maxElapsedTimeMs]) async {
    if (startingDelayMs.isNegative || startingDelayMs.bitLength > 32) {
      throw ArgumentError.value(
          startingDelayMs, 'startingDelayMs', 'Expected `u32`');
    }

    if (maxDelay.isNegative || maxDelay.bitLength > 32) {
      throw ArgumentError.value(maxDelay, 'maxDelay', 'Expected `u32`');
    }

    if (maxElapsedTimeMs != null) {
      if (maxElapsedTimeMs.isNegative || maxElapsedTimeMs.bitLength > 32) {
        throw ArgumentError.value(
            maxElapsedTimeMs, 'maxElapsedTimeMs', 'Expected `u32`');
      }
    }

    try {
      await (api.reconnectHandleReconnectWithBackoff(
          reconnectHandle: opaque.innerOpaque,
          startingDelay: startingDelayMs,
          multiplier: multiplier,
          maxDelay: maxDelay,
          maxElapsedTimeMs: maxElapsedTimeMs) as Future);
    } on FfiException catch (anyhow) {
      throw anyhow.parse();
    }
  }

  @moveSemantics
  @override
  void free() {
    if (!opaque.isStale()) {
      RustHandlesStorage().removeHandle(this);

      opaque.dispose();
    }
  }
}
