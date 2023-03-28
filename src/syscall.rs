#![allow(clippy::type_complexity)]

use paste::paste;

pub enum PassthroughVariant<A1, A2, A3, A4, A5, A6> {
    Func0(fn()),
    Func1(fn(A1) -> (A1,)),
    Func2(fn(A1, A2) -> (A1, A2)),
    Func3(fn(A1, A2, A3) -> (A1, A2, A3)),
    Func4(fn(A1, A2, A3, A4) -> (A1, A2, A3, A4)),
    Func5(fn(A1, A2, A3, A4, A5) -> (A1, A2, A3, A4, A5)),
    Func6(fn(A1, A2, A3, A4, A5, A6) -> (A1, A2, A3, A4, A5, A6)),
}

pub enum BlockVariant<R, A1, A2, A3, A4, A5, A6> {
    Func0(fn() -> R),
    Func1(fn(A1) -> R),
    Func2(fn(A1, A2) -> R),
    Func3(fn(A1, A2, A3) -> R),
    Func4(fn(A1, A2, A3, A4) -> R),
    Func5(fn(A1, A2, A3, A4, A5) -> R),
    Func6(fn(A1, A2, A3, A4, A5, A6) -> R),
}

pub enum Variant<R, A1, A2, A3, A4, A5, A6> {
    Passthrough(PassthroughVariant<A1, A2, A3, A4, A5, A6>),
    Block(BlockVariant<R, A1, A2, A3, A4, A5, A6>),
}

pub enum ReturnVariant<R, A1, A2, A3, A4, A5, A6> {
    PackedArgs(PackedArgs<A1, A2, A3, A4, A5, A6>),
    Normal(R),
}

type PackedArgs<A1, A2, A3, A4, A5, A6> = (
    Option<A1>,
    Option<A2>,
    Option<A3>,
    Option<A4>,
    Option<A5>,
    Option<A6>,
);

trait VariantInto<A1, A2, A3, A4, A5, A6> {
    fn to_pa(self) -> PackedArgs<A1, A2, A3, A4, A5, A6>;
}

macro_rules! impl_variant_into {
    ($($n: literal),* | $($m: tt),*) => {
        paste! {
            impl<A0, A1, A2, A3, A4, A5> VariantInto<A0, A1, A2, A3, A4, A5> for ($([<A $n>],)*) {
                fn to_pa(self) -> PackedArgs<A0, A1, A2, A3, A4, A5> {
                    ($(Some(self.$n),)* $($m,)*)
                }
            }
        }
    };
}

impl_variant_into!(| None, None, None, None, None, None);
impl_variant_into!(0 | None, None, None, None, None);
impl_variant_into!(0, 1 | None, None, None, None);
impl_variant_into!(0, 1, 2 | None, None, None);
impl_variant_into!(0, 1, 2, 3 | None, None);
impl_variant_into!(0, 1, 2, 3, 4 | None);
impl_variant_into!(0, 1, 2, 3, 4, 5 |);

pub struct SysCall<R, A1, A2, A3, A4, A5, A6> {
    pub name: &'static str,
    pub pre: Variant<R, A1, A2, A3, A4, A5, A6>,
    pub post: fn(R) -> R,
}

impl<R, A1, A2, A3, A4, A5, A6> SysCall<R, A1, A2, A3, A4, A5, A6> {
    pub fn call_pre(
        &self,
        a1: A1,
        a2: A2,
        a3: A3,
        a4: A4,
        a5: A5,
        a6: A6,
    ) -> ReturnVariant<R, A1, A2, A3, A4, A5, A6> {
        match &self.pre {
            Variant::Passthrough(pv) => ReturnVariant::PackedArgs(match pv {
                PassthroughVariant::Func0(f) => {
                    f();
                    ().to_pa()
                }
                PassthroughVariant::Func1(f) => f(a1).to_pa(),
                PassthroughVariant::Func2(f) => f(a1, a2).to_pa(),
                PassthroughVariant::Func3(f) => f(a1, a2, a3).to_pa(),
                PassthroughVariant::Func4(f) => f(a1, a2, a3, a4).to_pa(),
                PassthroughVariant::Func5(f) => f(a1, a2, a3, a4, a5).to_pa(),
                PassthroughVariant::Func6(f) => f(a1, a2, a3, a4, a5, a6).to_pa(),
            }),
            Variant::Block(bv) => ReturnVariant::Normal(match bv {
                BlockVariant::Func0(f) => f(),
                BlockVariant::Func1(f) => f(a1),
                BlockVariant::Func2(f) => f(a1, a2),
                BlockVariant::Func3(f) => f(a1, a2, a3),
                BlockVariant::Func4(f) => f(a1, a2, a3, a4),
                BlockVariant::Func5(f) => f(a1, a2, a3, a4, a5),
                BlockVariant::Func6(f) => f(a1, a2, a3, a4, a5, a6),
            }),
        }
    }

    pub fn call_post(&self, r: R) -> R {
        (self.post)(r)
    }
}

pub(crate) enum ReturnVariantWrapper {
    PackedArgs(
        (
            Option<u64>,
            Option<u64>,
            Option<u64>,
            Option<u64>,
            Option<u64>,
            Option<u64>,
        ),
    ),
    Normal(u64),
}

pub(crate) struct SysCallWrapper {
    pub(crate) name: &'static str,
    pub(crate) pre:
        Box<dyn Fn(&mut pete::Tracee, u64, u64, u64, u64, u64, u64) -> ReturnVariantWrapper>,
    pub(crate) post: Box<dyn Fn(u64) -> u64>,
}
