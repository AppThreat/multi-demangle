// Fixture source compiled by scripts/collect-swift-corpus.sh with each Swift
// toolchain whose mangling output we snapshot. Its (internal) symbols land in
// the object file's symbol table, where `nm` collects them into the
// per-version corpus under tests/corpus/swift/<version>/.
//
// The file deliberately exercises the constructs whose manglings drift
// between toolchains: generics and associated types, closures (escaping,
// autoclosure, @Sendable), async/await and throw, actors and global-actor
// isolation, property wrappers, subscripts, and specialization attributes.
//
// Swift 6.4 additions (SE-0507 borrow/mutate accessors, SE-0521 optional
// opaque/existential types, SE-0474 yielding accessors, SE-0493/SE-0504
// async-defer and cancellation shielding) are guarded with
// `#if compiler(>=6.4)` so the fixture still compiles with the 6.3.x
// toolchains that back the older corpora. The yielding accessors additionally
// need `-enable-experimental-feature CoroutineAccessors`, which
// collect-swift-corpus.sh passes automatically when the toolchain accepts it.

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

// --- Swift 6.4 ---

#if compiler(>=6.4)
// nonisolated(nonsending) parameter isolation: the closure type mangles with
// the NonIsolatedCallerFunctionType marker ('C').
actor Queue {
    func enqueue(_ op: nonisolated(nonsending) () async -> Void) async {
        await op()
    }
}
// SE-0507 borrow/mutate accessors on a Copyable type: the accessors mangle
// with the borrow ('b') and mutate ('z') accessor letters.
struct Gauge64 {
    private var backing: Int = 7
    var direct: Int {
        borrow { backing }
        mutate { &backing }
    }
}

// SE-0507 accessors on a ~Copyable type (the proposal's motivating form).
struct Rigid<Element: ~Copyable>: ~Copyable {
    var _element: Element
    var element: Element {
        borrow {
            return _element
        }
        mutate {
            return &_element
        }
    }
}

// SE-0521: optional opaque and existential types without parentheses.
func optionalOpaque64(_ p: Packet) -> some Payload? { p }
func optionalAny64(_ p: (any Payload)?) -> any Payload? { p }

// SE-0474 yielding accessors (experimental in 6.4): the coroutine accessors
// mangle as YieldingBorrowAccessor ('y') / YieldingMutateAccessor ('x').
struct Yielder {
    private var storage: [Int] = [1, 2, 3]
    var current: Int {
        yielding borrow { yield storage[0] }
        yielding mutate { yield &storage[0] }
    }
}

// SE-0493: defer in async code runs (and is awaited) before the function
// exits. No dedicated mangling, but it exercises 6.4 codegen for the corpus.
func deferredAsync(_ flag: Bool) async -> Int {
    defer { print("exiting") }
    return flag ? 1 : 0
}
#endif
