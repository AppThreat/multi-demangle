/**
 * D corpus fixture.
 *
 * src/dlang.rs is the crate's largest hand-written parser (~1750 lines) and
 * implements the full D ABI type grammar on top of LLVM's basic-types-only
 * demangler. Every construct below maps onto a piece of that grammar, so
 * `nm` over this module produces the symbols that exercise it.
 *
 * D is the one new language with a mature independent oracle: GNU `c++filt`
 * embeds libiberty's `d-demangle.c`. Every symbol collected here can be run
 * through both implementations and diffed — see contrib/collect-corpus.sh.
 */
module corpus;

// --- Basic function types, return types, linkage ---------------------------

void voidFunc() {}
int intFunc(int x) { return x; }
double manyArgs(int a, long b, float c, char d, bool e) { return 0; }
void variadic(int a, ...) {}
extern (C) void cLinkage() {}          // not mangled: a negative control
extern (C++) void cppLinkage() {}      // Itanium-mangled: another control

// --- Compound and qualified types -----------------------------------------

int[] dynamicArray(int[] a) { return a; }
int[4] staticArray(int[4] a) { return a; }
int[string] assocArray(int[string] a) { return a; }
int* pointer(int* p) { return p; }
const(int) constArg(const(int) x) { return x; }
immutable(char)[] immutableString(immutable(char)[] s) { return s; }
shared(int) sharedArg(shared(int) x) { return x; }
inout(int)[] inoutArg(inout(int)[] x) { return x; }
void nestedCompound(const(int*)[][string] x) {}

// --- Delegates and function pointers ---------------------------------------

void takesDelegate(int delegate(int) dg) {}
void takesFunctionPointer(int function(int) fp) {}
int delegate(int) returnsDelegate() { return null; }

// --- Aggregates: struct, class, interface, union, enum ---------------------

struct S {
    int field;
    void method() {}                    // member function -> the `M` marker
    const void constMethod() {}
    static void staticMethod() {}
    this(int x) { field = x; }          // constructor
    ~this() {}                          // destructor
    this(this) {}                       // postblit
    int opBinary(string op)(int rhs) { return rhs; }
    @property int prop() { return field; }
}

class C {
    int field;
    void method() {}
    final void finalMethod() {}
    static void staticMethod() {}
    override string toString() { return "C"; }
    this() {}
    ~this() {}
}

class Derived : C {
    override void method() {}
}

interface I {
    void interfaceMethod();
}

union U {
    int i;
    float f;
}

enum E { a, b, c }
void takesEnum(E e) {}
void takesAggregates(S s, C c, I i, U u) {}

// --- Templates: the `__T` instance grammar ---------------------------------

T identity(T)(T value) { return value; }
void twoParams(T, U)(T a, U b) {}
T withValue(T, int N)(T x) { return x; }        // value template argument
void withString(string s)() {}                  // string template argument
void withAlias(alias F)() {}

struct TemplatedStruct(T) {
    T value;
    void method() {}
    T get() { return value; }
}

class TemplatedClass(T, U) {
    void method(T t, U u) {}
}

// Force instantiation so the symbols are actually emitted.
void instantiate() {
    identity!int(1);
    identity!string("x");
    identity!(int[])(null);
    identity!(S)(S(1));
    twoParams!(int, string)(1, "x");
    withValue!(int, 42)(1);
    withString!("hello")();
    TemplatedStruct!int ts;
    ts.method();
    ts.get();
    TemplatedStruct!(int[string]) ts2;
    ts2.method();
    auto tc = new TemplatedClass!(int, string)();
    tc.method(1, "x");
}

// --- Nested functions and closures -----------------------------------------

void outerFunction() {
    void nestedFunction() {}
    int nestedWithArgs(int x) { return x; }
    nestedFunction();
    nestedWithArgs(1);
}

// --- Module-level data, ctors, unittests -----------------------------------

int moduleVariable;
__gshared int gsharedVariable;
static this() {}                        // module constructor
static ~this() {}                       // module destructor
shared static this() {}

unittest {
    assert(intFunc(1) == 1);
}

// --- A deliberately long qualified path ------------------------------------

struct Outer {
    struct Middle {
        struct Inner {
            void deeplyNested() {}
        }
    }
}

void main() {
    instantiate();
    outerFunction();
}
