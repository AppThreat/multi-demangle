//! CodeWarrior demangler integration tests derived from upstream crate tests.

use multi_demangle::{Demangle, DemangleOptions};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name, NameMangling};

fn assert_demangle_variants(symbol: &str, full: &str, no_return: &str, name_only: &str) {
    let name = Name::new(symbol, NameMangling::Unknown, Language::Cpp);
    assert_eq!(name.demangle(DemangleOptions::complete()), Some(full.to_string()));
    assert_eq!(
        name.demangle(DemangleOptions::complete().return_type(false)),
        Some(no_return.to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::name_only()),
        Some(name_only.to_string())
    );
}

#[test]
fn test_demangle_codewarrior_symbols() {
    for (symbol, full, no_return, name_only) in [
        (
            "BuildLight__9CGuiLightCFv",
            "CGuiLight::BuildLight() const",
            "CGuiLight::BuildLight() const",
            "CGuiLight::BuildLight",
        ),
        (
            "__pl__FRC9CRelAngleRC9CRelAngle",
            "operator+(CRelAngle const &, CRelAngle const &)",
            "operator+(CRelAngle const &, CRelAngle const &)",
            "operator+",
        ),
        (
            "__dt__6CActorFv",
            "CActor::~CActor()",
            "CActor::~CActor()",
            "CActor::~CActor",
        ),
        (
            "SomeFn__FRCPFPFPCvPv_v_RCPFPCvPv_v",
            "SomeFn(void (*const &(*const &)(void (*)(void const *, void *)))(void const *, void *))",
            "SomeFn(void (*const &(*const &)(void (*)(void const *, void *)))(void const *, void *))",
            "SomeFn",
        ),
        (
            "SomeFn__Q29Namespace5ClassCFRCMQ29Namespace5ClassFPCvPCvMQ29Namespace5ClassFPCvPCvPCvPv_v_RCMQ29Namespace5ClassFPCvPCvPCvPv_v",
            "Namespace::Class::SomeFn(void (Namespace::Class::*const & (Namespace::Class::*const &)(void (Namespace::Class::*)(const void*, void*) const) const)(const void*, void*) const) const",
            "Namespace::Class::SomeFn(void (Namespace::Class::*const & (Namespace::Class::*const &)(void (Namespace::Class::*)(const void*, void*) const) const)(const void*, void*) const) const",
            "Namespace::Class::SomeFn",
        ),
        (
            "execCommand__12JASSeqParserFP8JASTrackM12JASSeqParserFPCvPvP8JASTrackPUl_lUlPUl",
            "JASSeqParser::execCommand(JASTrack*, long (JASSeqParser::*)(JASTrack*, unsigned long*), unsigned long, unsigned long*)",
            "JASSeqParser::execCommand(JASTrack*, long (JASSeqParser::*)(JASTrack*, unsigned long*), unsigned long, unsigned long*)",
            "JASSeqParser::execCommand",
        ),
        (
            "AddWidgetFnMap__10CGuiWidgetFiM10CGuiWidgetFPCvPvP15CGuiFunctionDefP18CGuiControllerInfo_i",
            "CGuiWidget::AddWidgetFnMap(int, int (CGuiWidget::*)(CGuiFunctionDef*, CGuiControllerInfo*))",
            "CGuiWidget::AddWidgetFnMap(int, int (CGuiWidget::*)(CGuiFunctionDef*, CGuiControllerInfo*))",
            "CGuiWidget::AddWidgetFnMap",
        ),
        (
            "BareFn__FPFPCcPv_v_PFPCvPv_v",
            "void (* BareFn(void (*)(const char*, void*)))(const void*, void*)",
            "BareFn(void (*)(const char*, void*))",
            "BareFn",
        ),
    ] {
        assert_demangle_variants(symbol, full, no_return, name_only);
    }
}
