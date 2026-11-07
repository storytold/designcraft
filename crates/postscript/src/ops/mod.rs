//! The operator table: every name `systemdict` binds to an operator.

pub mod basic;
pub mod gfx;
pub mod image;
pub mod mesh;
pub mod misc;
pub mod shading;

use crate::interp::{Interp, R};
use crate::obj::{Obj, PArr, PDict};
use crate::{composite, font};

pub type OpId = u16;
pub type OpFn = fn(&mut Interp) -> R;

macro_rules! named {
    ($($n:literal => $f:expr),* $(,)?) => { &[$(($n.as_bytes(), $f as OpFn)),*] };
}

pub static OPS: &[(&[u8], OpFn)] = named! {
    // stack
    "pop" => basic::pop, "exch" => basic::exch, "dup" => basic::dup, "copy" => basic::copy, "index" => basic::index,
    "roll" => basic::roll, "clear" => basic::clear, "count" => basic::count, "mark" => basic::mark, "[" => basic::mark,
    "<<" => basic::mark, "]" => basic::array_close, ">>" => basic::dict_close, "cleartomark" => basic::cleartomark,
    "counttomark" => basic::counttomark,
    // arithmetic
    "add" => basic::add, "sub" => basic::sub, "mul" => basic::mul, "div" => basic::div, "idiv" => basic::idiv,
    "mod" => basic::mod_, "neg" => basic::neg, "abs" => basic::abs, "ceiling" => basic::ceiling, "floor" => basic::floor,
    "round" => basic::round, "truncate" => basic::truncate, "sqrt" => basic::sqrt, "sin" => basic::sin, "cos" => basic::cos,
    "atan" => basic::atan, "exp" => basic::exp, "ln" => basic::ln, "log" => basic::log, "rand" => basic::rand,
    "srand" => basic::srand, "rrand" => basic::rrand,
    // relations
    "eq" => basic::eq, "ne" => basic::ne, "gt" => basic::gt, "ge" => basic::ge, "lt" => basic::lt, "le" => basic::le,
    "and" => basic::and, "or" => basic::or, "xor" => basic::xor, "not" => basic::not, "bitshift" => basic::bitshift,
    "true" => basic::true_, "false" => basic::false_, "null" => basic::null,
    // control
    "exec" => basic::exec, "if" => basic::if_, "ifelse" => basic::ifelse, "for" => basic::for_, "repeat" => basic::repeat,
    "loop" => basic::loop_, "exit" => basic::exit, "stop" => basic::stop, "stopped" => basic::stopped, "quit" => basic::quit,
    "forall" => basic::forall, "countexecstack" => basic::countexecstack, "execstack" => basic::execstack,
    // types
    "type" => basic::type_, "cvlit" => basic::cvlit, "cvx" => basic::cvx, "xcheck" => basic::xcheck,
    "readonly" => basic::noop_keep, "executeonly" => basic::noop_keep, "noaccess" => basic::noop_keep,
    "rcheck" => basic::check_true, "wcheck" => basic::check_true, "cvi" => basic::cvi, "cvr" => basic::cvr,
    "cvn" => basic::cvn, "cvs" => basic::cvs, "cvrs" => basic::cvrs, "bind" => basic::bind,
    // dictionaries
    "dict" => basic::dict, "maxlength" => basic::maxlength, "begin" => basic::begin, "end" => basic::end, "def" => basic::def,
    "load" => basic::load, "store" => basic::store, "get" => basic::get, "put" => basic::put, "known" => basic::known,
    "undef" => basic::undef, "where" => basic::where_, "currentdict" => basic::currentdict,
    "countdictstack" => basic::countdictstack, "dictstack" => basic::dictstack, "cleardictstack" => basic::cleardictstack,
    // strings and arrays
    "array" => basic::array, "string" => basic::string, "length" => basic::length, "getinterval" => basic::getinterval,
    "putinterval" => basic::putinterval, "aload" => basic::aload, "astore" => basic::astore,
    "packedarray" => basic::packedarray, "search" => basic::search, "anchorsearch" => basic::anchorsearch,
    "token" => basic::token,
    // graphics state
    "gsave" => gfx::gsave, "grestore" => gfx::grestore, "grestoreall" => gfx::grestoreall, "initgraphics" => gfx::initgraphics,
    "save" => gfx::save, "restore" => gfx::restore, "gstate" => gfx::gstate, "currentgstate" => gfx::currentgstate,
    "setgstate" => gfx::setgstate, "setlinewidth" => gfx::setlinewidth, "currentlinewidth" => gfx::currentlinewidth,
    "setlinecap" => gfx::setlinecap, "currentlinecap" => gfx::currentlinecap, "setlinejoin" => gfx::setlinejoin,
    "currentlinejoin" => gfx::currentlinejoin, "setmiterlimit" => gfx::setmiterlimit,
    "currentmiterlimit" => gfx::currentmiterlimit, "setdash" => gfx::setdash, "currentdash" => gfx::currentdash,
    "setflat" => gfx::setflat, "currentflat" => gfx::currentflat, "setstrokeadjust" => gfx::setstrokeadjust,
    "currentstrokeadjust" => gfx::currentstrokeadjust, "setoverprint" => gfx::setoverprint,
    "currentoverprint" => gfx::currentoverprint,
    // matrices
    "matrix" => gfx::matrix, "initmatrix" => gfx::initmatrix, "identmatrix" => gfx::identmatrix,
    "defaultmatrix" => gfx::defaultmatrix, "currentmatrix" => gfx::currentmatrix, "setmatrix" => gfx::setmatrix,
    "translate" => gfx::translate, "scale" => gfx::scale, "rotate" => gfx::rotate, "concat" => gfx::concat,
    "concatmatrix" => gfx::concatmatrix, "invertmatrix" => gfx::invertmatrix, "transform" => gfx::transform,
    "itransform" => gfx::itransform, "dtransform" => gfx::dtransform, "idtransform" => gfx::idtransform,
    // paths
    "newpath" => gfx::newpath, "moveto" => gfx::moveto, "rmoveto" => gfx::rmoveto, "lineto" => gfx::lineto,
    "rlineto" => gfx::rlineto, "curveto" => gfx::curveto, "rcurveto" => gfx::rcurveto, "closepath" => gfx::closepath,
    "arc" => gfx::arc, "arcn" => gfx::arcn, "arct" => gfx::arct, "arcto" => gfx::arcto, "currentpoint" => gfx::currentpoint,
    "flattenpath" => gfx::flattenpath, "reversepath" => gfx::reversepath, "strokepath" => gfx::strokepath,
    "pathbbox" => gfx::pathbbox, "pathforall" => gfx::pathforall, "clippath" => gfx::clippath, "initclip" => gfx::initclip,
    "clipsave" => gfx::clipsave, "cliprestore" => gfx::cliprestore, "clip" => gfx::clip, "eoclip" => gfx::eoclip, "rectclip" => gfx::rectclip,
    // painting
    "fill" => gfx::fill, "eofill" => gfx::eofill, "stroke" => gfx::stroke, "rectfill" => gfx::rectfill,
    "rectstroke" => gfx::rectstroke, "shfill" => shading::shfill, "erasepage" => misc::noop,
    "image" => image::image, "colorimage" => image::colorimage, "imagemask" => image::imagemask,
    // colour
    "setgray" => gfx::setgray, "currentgray" => gfx::currentgray, "setrgbcolor" => gfx::setrgbcolor,
    "currentrgbcolor" => gfx::currentrgbcolor, "sethsbcolor" => gfx::sethsbcolor, "currenthsbcolor" => gfx::currenthsbcolor,
    "setcmykcolor" => gfx::setcmykcolor, "currentcmykcolor" => gfx::currentcmykcolor, "setcolorspace" => gfx::setcolorspace,
    "currentcolorspace" => gfx::currentcolorspace, "setcolor" => gfx::setcolor, "currentcolor" => gfx::currentcolor,
    "setpattern" => gfx::setpattern, "makepattern" => gfx::makepattern,
    // fonts
    "findfont" => font::findfont, "definefont" => font::definefont, "undefinefont" => font::undefinefont,
    "scalefont" => font::scalefont, "makefont" => font::makefont, "setfont" => font::setfont,
    "currentfont" => font::currentfont, "rootfont" => font::currentfont, "selectfont" => font::selectfont,
    "show" => font::show, "ashow" => font::ashow, "widthshow" => font::widthshow, "awidthshow" => font::awidthshow,
    "xshow" => font::xshow, "yshow" => font::yshow, "xyshow" => font::xyshow, "kshow" => font::kshow, "cshow" => font::cshow,
    "glyphshow" => font::glyphshow, "stringwidth" => font::stringwidth, "charpath" => font::charpath,
    "setcharwidth" => font::setcharwidth, "setcachedevice" => font::setcachedevice,
    "setcachedevice2" => font::setcachedevice2, "setcachelimit" => misc::pop1, "cachestatus" => misc::noop,
    // files
    "currentfile" => misc::currentfile, "read" => misc::read, "readstring" => misc::readstring,
    "readhexstring" => misc::readhexstring, "readline" => misc::readline, "bytesavailable" => misc::bytesavailable,
    "closefile" => misc::closefile, "resetfile" => misc::resetfile, "fileposition" => misc::fileposition,
    "setfileposition" => misc::setfileposition, "flushfile" => misc::flushfile, "status" => misc::status,
    "file" => misc::file, "write" => misc::write_ops, "writestring" => misc::write_ops, "writehexstring" => misc::write_ops,
    "eexec" => misc::eexec, "filter" => misc::filter, "flush" => misc::noop,
    "print" => misc::print_any, "=" => misc::print_any, "==" => misc::print_any, "stack" => misc::pstack,
    "pstack" => misc::pstack,
    // resources and forms
    "findresource" => misc::findresource, "defineresource" => misc::defineresource,
    "undefineresource" => misc::undefineresource, "resourcestatus" => misc::resourcestatus,
    "resourceforall" => misc::resourceforall, "execform" => misc::execform,
    // user paths and objects
    "ucache" => misc::ucache, "setbbox" => misc::setbbox, "uappend" => misc::uappend, "ufill" => misc::ufill,
    "ueofill" => misc::ueofill, "ustroke" => misc::ustroke, "defineuserobject" => misc::defineuserobject,
    "execuserobject" => misc::execuserobject, "undefineuserobject" => misc::undefineuserobject,
    // environment and device
    "showpage" => misc::showpage, "copypage" => misc::noop, "nulldevice" => misc::nulldevice,
    "languagelevel" => misc::languagelevel, "version" => misc::version, "product" => misc::product,
    "revision" => misc::revision, "serialnumber" => misc::zero, "realtime" => misc::realtime, "usertime" => misc::realtime,
    "vmstatus" => misc::vmstatus, "vmreclaim" => misc::pop1, "setvmthreshold" => misc::pop1,
    "setglobal" => misc::setglobal, "currentglobal" => misc::push_false, "setpacking" => misc::setglobal,
    "currentpacking" => misc::push_false, "gcheck" => misc::gcheck, "setshared" => misc::setglobal,
    "currentshared" => misc::push_false, "setpagedevice" => misc::pop_dict, "currentpagedevice" => misc::currentpagedevice,
    "setuserparams" => misc::pop_dict, "currentuserparams" => misc::currentuserparams, "setsystemparams" => misc::pop_dict,
    "currentsystemparams" => misc::currentsystemparams, "setdevparams" => misc::write_ops, "currentdevparams" => misc::empty_dict,
    "setscreen" => misc::setscreen, "currentscreen" => misc::currentscreen, "setcolorscreen" => misc::setcolorscreen,
    "currentcolorscreen" => misc::currentcolorscreen, "sethalftone" => misc::pop_dict, "currenthalftone" => misc::empty_dict,
    "settransfer" => misc::pop1, "currenttransfer" => misc::empty_proc, "setcolortransfer" => misc::setcolortransfer,
    "currentcolortransfer" => misc::currentcolortransfer, "setblackgeneration" => misc::pop1,
    "currentblackgeneration" => misc::empty_proc, "setundercolorremoval" => misc::pop1,
    "currentundercolorremoval" => misc::empty_proc, "setcolorrendering" => misc::pop_dict,
    "currentcolorrendering" => misc::empty_dict, "findcolorrendering" => misc::findcolorrendering,
    "setsmoothness" => misc::string_noop, "currentsmoothness" => misc::zero, "internaldict" => misc::internaldict,
    "setucacheparams" => misc::noop, "ucachestatus" => misc::noop, "setobjectformat" => misc::pop1,
    "currentobjectformat" => misc::zero, "sethalftonephase" => misc::setbbox, "echo" => misc::pop1,
    "prompt" => misc::noop, "executive" => misc::noop, "start" => misc::noop,
    // CFF font sets, CMaps and composite fonts
    "StartData" => composite::startdata, "composefont" => composite::composefont,
    "begincmap" => composite::begincmap, "endcmap" => composite::endcmap, "usecmap" => composite::usecmap,
    "begincodespacerange" => composite::begin_block, "endcodespacerange" => composite::endcodespacerange,
    "begincidrange" => composite::begin_block, "endcidrange" => composite::endcidrange,
    "begincidchar" => composite::begin_block, "endcidchar" => composite::endcidchar,
    "beginbfchar" => composite::begin_block, "endbfchar" => composite::end_ignored,
    "beginbfrange" => composite::begin_block, "endbfrange" => composite::end_ignored,
    "beginnotdefrange" => composite::begin_block, "endnotdefrange" => composite::end_ignored,
    "beginnotdefchar" => composite::begin_block, "endnotdefchar" => composite::end_ignored,
};

pub fn op_id(n: &[u8]) -> Option<OpId> {
    OPS.iter().position(|(name, _)| *name == n).and_then(|i| OpId::try_from(i).ok())
}

fn encoding(names: Vec<&'static str>) -> Obj {
    Obj::Array(PArr::new(names.into_iter().map(|n| Obj::lit(n.as_bytes())).collect()), false)
}

/// Fill systemdict and the standard dictionaries.
pub fn install(ip: &mut Interp) {
    for (i, (n, _)) in OPS.iter().enumerate() {
        if let Ok(id) = OpId::try_from(i) {
            ip.systemdict.put_name(n, Obj::Op(id));
        }
    }
    // Environment queries are plain values: executing the name pushes them, and programs that
    // `get` them from systemdict (`systemdict /languagelevel get 2 ge`) find a number.
    ip.systemdict.put_name(b"languagelevel", Obj::Int(3));
    for (n, d) in [
        (&b"systemdict"[..], ip.systemdict.clone()),
        (b"userdict", ip.userdict.clone()),
        (b"globaldict", ip.globaldict.clone()),
        (b"errordict", ip.errordict.clone()),
        (b"$error", ip.dollar_error.clone()),
        (b"statusdict", ip.statusdict.clone()),
        (b"FontDirectory", ip.fontdir.clone()),
        (b"GlobalFontDirectory", ip.fontdir.clone()),
        (b"SharedFontDirectory", ip.fontdir.clone()),
    ] {
        ip.systemdict.put_name(n, Obj::Dict(d));
    }
    ip.systemdict.put_name(b"version", Obj::str(b"3010"));
    ip.systemdict.put_name(b"product", Obj::str(b"DesignCraft"));
    ip.systemdict.put_name(b"revision", Obj::Int(1));
    ip.systemdict.put_name(b"serialnumber", Obj::Int(0));
    ip.systemdict.put_name(b"StandardEncoding", encoding(crate::encodings::standard()));
    ip.systemdict.put_name(b"ISOLatin1Encoding", encoding(crate::encodings::iso_latin1()));
    // Errors are handled by `stopped`; the handlers exist for programs that look at them.
    for e in [
        "dictfull",
        "dictstackoverflow",
        "dictstackunderflow",
        "execstackoverflow",
        "handleerror",
        "interrupt",
        "invalidaccess",
        "invalidexit",
        "invalidfileaccess",
        "invalidfont",
        "invalidrestore",
        "ioerror",
        "limitcheck",
        "nocurrentpoint",
        "rangecheck",
        "stackoverflow",
        "stackunderflow",
        "syntaxerror",
        "timeout",
        "typecheck",
        "undefined",
        "undefinedfilename",
        "undefinedresult",
        "unmatchedmark",
        "unregistered",
        "VMerror",
        "configurationerror",
        "undefinedresource",
    ] {
        let stop = op_id(b"stop").map(Obj::Op).unwrap_or(Obj::Null);
        ip.errordict.put_name(e.as_bytes(), Obj::proc(vec![stop]));
    }
    ip.dollar_error.put_name(b"newerror", Obj::Bool(false));
    ip.statusdict.put_name(b"product", Obj::str(b"DesignCraft"));
    ip.statusdict.put_name(b"processcolors", Obj::proc(vec![Obj::Int(4)]));
    ip.userdict.put_name(b"#copies", Obj::Int(1));
    let cats = PDict::new(4);
    ip.resources.put_name(b"Category", Obj::Dict(cats));
    composite::install(ip);
}
