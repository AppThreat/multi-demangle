// Kotlin/Native corpus fixture.
//
// src/kotlin_native.rs parses the readable `kfun:` spelling modern
// Kotlin/Native emits: a dotted package path, `#`-separated member names and
// compiler markers, a `;`-separated parameter list, a braced type-parameter
// block, and a return type. Every declaration here maps onto one of those
// pieces (plus the compiler-generated accessors, bridges, and trampolines
// the declarations cause).
//
// Scope answer, verified with the 2.0.21 prebuilt compiler: `kfun:` is still
// emitted, in volume — this small fixture produces ~950 of them. The dotted
// `kfun:com.example.Foo.bar(...)` form shown in the 2018 GitHub issue the
// backend was originally written from is NOT what current compilers emit.

package com.example

// Top-level functions: the `main(kotlin.Array<kotlin.String>)` shape.
fun main(args: Array<String>) {
    val c = Counter()
    c.increment(1)
    println(describe(c.value, "count"))
    genericIdentity(1)
    genericIdentity("x")
    nullableArg(null)
    defaulted()
}

fun describe(n: Int, label: String): String = "$label=$n"

// Nullable types must survive the rendering (`kotlin.Any?` -> `Any?`).
fun nullableArg(x: String?): Int? = x?.length

// Generics, including nested ones, which exercise the `<...>` handling.
fun <T> genericIdentity(value: T): T = value
fun mapOfLists(m: Map<String, List<Int>>): Set<Map.Entry<String, List<Int>>> = m.entries

// Default arguments generate an extra `$default` bridge symbol.
fun defaulted(a: Int = 1, b: String = "x"): String = "$a$b"

// A class with methods, properties, a companion, and an init block.
class Counter {
    var value: Int = 0
        private set

    fun increment(by: Int): Int {
        value += by
        return value
    }

    fun reset() {
        value = 0
    }

    companion object {
        fun create(): Counter = Counter()
    }
}

// Inheritance and interfaces: virtual dispatch produces extra symbols.
interface Shape {
    fun area(): Double
}

open class Rect(val w: Double, val h: Double) : Shape {
    override fun area(): Double = w * h
}

class Square(side: Double) : Rect(side, side)

// A data class: generates equals/hashCode/toString/copy/componentN.
data class Point(val x: Int, val y: Int)

// An object declaration and an enum.
object Registry {
    fun register(name: String) {}
}

enum class Color { RED, GREEN, BLUE }

// An extension function on a stdlib type — the receiver shows up in the
// mangled name and is a good test of the qualified-name split.
fun String.shout(): String = this.uppercase()

// A package-level property with a custom getter.
val computed: Int
    get() = 42
