// Fixture source compiled by scripts/collect-swift-corpus.sh with each Swift
// toolchain whose mangling output we snapshot. Its (internal) symbols land in
// the object file's symbol table, where `nm` collects them into the
// per-version corpus under tests/corpus/swift/<version>/.
//
// The file deliberately exercises the constructs whose manglings drift
// between toolchains: generics and associated types, closures (escaping,
// autoclosure, @Sendable), async/await and throw, actors and global-actor
// isolation, property wrappers, subscripts, and specialization attributes.

import Foundation

protocol Payload {
    associatedtype Value
    var value: Value { get }
}

struct Packet: Payload, Sendable {
    var value: Int
    let origin: String
}

enum Outcome<T: Payload> {
    case delivered(T)
    case bounced(reason: String)

    var isDelivered: Bool { if case .delivered = self { return true }; return false }
}

actor Meter {
    private var ticks = 0
    func tick(by step: Int = 1) -> Int {
        ticks += step
        return ticks
    }
    nonisolated var kind: String { "meter" }
    func drain() async -> Int { ticks }
}

@MainActor
final class Dashboard {
    private var readings: [String: Int] = [:]
    var count: Int { readings.count }

    func record(_ key: String, value: Int) {
        readings[key] = value
    }

    func sweep() async -> [String] {
        await withTaskGroup(of: String.self) { group in
            for key in readings.keys {
                group.addTask { key.uppercased() }
            }
            var names: [String] = []
            for await name in group {
                names.append(name)
            }
            return names.sorted()
        }
    }
}

func measure<T: Payload>(_ item: T, times n: Int) -> T.Value where T.Value: Numeric {
    var total = T.Value.zero
    for _ in 0..<max(n, 0) {
        total += item.value
    }
    return total
}

func plain(_ x: Int, label y: String) -> Bool { x > y.count }

func variadic(_ xs: Int..., sep: String) -> String { xs.map(String.init).joined(separator: sep) }

func throwsOnError(code: Int) throws -> Int {
    guard code >= 0 else { throw CouchError.negative }
    return code
}

enum CouchError: Error { case negative }

func fetch(_ url: String) async throws -> Data {
    try await Task.sleep(nanoseconds: 1_000)
    return Data(url.utf8)
}

func makeSendableCounter() -> @Sendable (Int) async -> Int {
    { base in
        await withCheckedContinuation { continuation in
            continuation.resume(returning: base * 2)
        }
    }
}

func autoclosureWrap(_ body: @autoclosure () -> Int) -> Int { body() }

func rethrowingBridge(_ body: () throws -> Int, transform: (Int) -> Int) rethrows -> Int {
    transform(try body())
}

infix operator <>: AdditionPrecedence
func <><T: Numeric>(lhs: T, rhs: T) -> T { lhs + rhs }

@propertyWrapper
struct Clamped<Value: Comparable> {
    var wrappedValue: Value
    let range: ClosedRange<Value>

    init(wrappedValue: Value, _ range: ClosedRange<Value>) {
        self.range = range
        self.wrappedValue = min(max(wrappedValue, range.lowerBound), range.upperBound)
    }
}

struct Gauge {
    @Clamped(0...100) var level: Int = 50
    subscript(index: Int) -> String {
        "gauge[\(index)]"
    }
}

func opaqueTransfer(_ p: some Payload) -> some Payload { p }

func existentialAny(_ p: any Payload) -> any Payload { p }

func firstOfPack<each P: Payload2>(_ pack: repeat each P) -> Int { 0 }

protocol Payload2 {}

struct Nested {
    struct Inner {
        func deep(_ flag: Bool) -> Int { flag ? 1 : 0 }
    }
    func shallow() -> Inner { Inner() }
}

func buildDeepClosure() -> () -> () -> Int {
    return {
        let outer = 10
        return {
            outer + 1
        }
    }
}

extension Packet: CustomStringConvertible {
    var description: String { "packet(\(value))" }
}

@discardableResult
func discardable(_ flag: Bool) -> Int { flag ? 1 : 0 }
