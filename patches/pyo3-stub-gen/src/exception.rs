use pyo3::exceptions::*;

/// Wrapper of [pyo3::create_exception] macro to create a custom exception with [crate::PyStubType] support.
///
/// Note
/// -----
/// [pyo3::create_exception!] macro creates a new exception type as [pyo3::PyErr],
/// which does not implement [pyo3::PyClass] trait. So it is not a "class" in PyO3 sense,
/// but we create a [crate::type_info::PyClassInfo] since it will be treated as a class eventually in Python side.
#[macro_export]
macro_rules! create_exception {
    ($module: expr, $name: ident, $base: ty) => {
        $crate::create_exception!($module, $name, $base, "");
    };
    ($module: expr, $name: ident, $base: ty, $doc: expr) => {
        ::pyo3::create_exception!($module, $name, $base, $doc);

        // Add PyStubType implementation for the created exception
        impl $crate::PyStubType for $name {
            fn type_output() -> $crate::TypeInfo {
                $crate::TypeInfo::builtin(stringify!($name))
            }
        }

        $crate::inventory::submit! {
            $crate::type_info::PyClassInfo {
                pyclass_name: stringify!($name),
                struct_id: std::any::TypeId::of::<$name>,
                getters: &[],
                setters: &[],
                module: Some(stringify!($module)),
                doc: $doc,
                bases: &[|| <$base as $crate::PyStubType>::type_output()],
                has_eq: false,
                has_ord: false,
                has_hash: false,
                has_str: false,
                subclass: true,
            }
        }
    };
}

// Direct PyStubType implementations for PyO3 exception types
macro_rules! impl_exception_stub_type {
    ($name:ident, $type_name:literal) => {
        impl crate::PyStubType for $name {
            fn type_output() -> crate::TypeInfo {
                crate::TypeInfo::builtin($type_name)
            }
        }
    };
}

impl_exception_stub_type!(PyArithmeticError, "ArithmeticError");
impl_exception_stub_type!(PyAssertionError, "AssertionError");
impl_exception_stub_type!(PyAttributeError, "AttributeError");
impl_exception_stub_type!(PyBaseException, "BaseException");
impl_exception_stub_type!(PyBlockingIOError, "BlockingIOError");
impl_exception_stub_type!(PyBrokenPipeError, "BrokenPipeError");
impl_exception_stub_type!(PyBufferError, "BufferError");
impl_exception_stub_type!(PyBytesWarning, "BytesWarning");
impl_exception_stub_type!(PyChildProcessError, "ChildProcessError");
impl_exception_stub_type!(PyConnectionAbortedError, "ConnectionAbortedError");
impl_exception_stub_type!(PyConnectionError, "ConnectionError");
impl_exception_stub_type!(PyConnectionRefusedError, "ConnectionRefusedError");
impl_exception_stub_type!(PyConnectionResetError, "ConnectionResetError");
impl_exception_stub_type!(PyDeprecationWarning, "DeprecationWarning");
impl_exception_stub_type!(PyEOFError, "EOFError");
#[cfg(Py_3_10)]
impl_exception_stub_type!(PyEncodingWarning, "EncodingWarning");
impl_exception_stub_type!(PyException, "Exception");
impl_exception_stub_type!(PyFileExistsError, "FileExistsError");
impl_exception_stub_type!(PyFileNotFoundError, "FileNotFoundError");
impl_exception_stub_type!(PyFloatingPointError, "FloatingPointError");
impl_exception_stub_type!(PyFutureWarning, "FutureWarning");
impl_exception_stub_type!(PyGeneratorExit, "GeneratorExit");
impl_exception_stub_type!(PyImportError, "ImportError");
impl_exception_stub_type!(PyImportWarning, "ImportWarning");
impl_exception_stub_type!(PyIndexError, "IndexError");
impl_exception_stub_type!(PyInterruptedError, "InterruptedError");
impl_exception_stub_type!(PyIsADirectoryError, "IsADirectoryError");
impl_exception_stub_type!(PyKeyError, "KeyError");
impl_exception_stub_type!(PyKeyboardInterrupt, "KeyboardInterrupt");
impl_exception_stub_type!(PyLookupError, "LookupError");
impl_exception_stub_type!(PyMemoryError, "MemoryError");
impl_exception_stub_type!(PyModuleNotFoundError, "ModuleNotFoundError");
impl_exception_stub_type!(PyNameError, "NameError");
impl_exception_stub_type!(PyNotADirectoryError, "NotADirectoryError");
impl_exception_stub_type!(PyNotImplementedError, "NotImplementedError");
impl_exception_stub_type!(PyOSError, "OSError");
impl_exception_stub_type!(PyOverflowError, "OverflowError");
impl_exception_stub_type!(PyPendingDeprecationWarning, "PendingDeprecationWarning");
impl_exception_stub_type!(PyPermissionError, "PermissionError");
impl_exception_stub_type!(PyProcessLookupError, "ProcessLookupError");
impl_exception_stub_type!(PyRecursionError, "RecursionError");
impl_exception_stub_type!(PyReferenceError, "ReferenceError");
impl_exception_stub_type!(PyResourceWarning, "ResourceWarning");
impl_exception_stub_type!(PyRuntimeError, "RuntimeError");
impl_exception_stub_type!(PyRuntimeWarning, "RuntimeWarning");
impl_exception_stub_type!(PyStopAsyncIteration, "StopAsyncIteration");
impl_exception_stub_type!(PyStopIteration, "StopIteration");
impl_exception_stub_type!(PySyntaxError, "SyntaxError");
impl_exception_stub_type!(PySyntaxWarning, "SyntaxWarning");
impl_exception_stub_type!(PySystemError, "SystemError");
impl_exception_stub_type!(PySystemExit, "SystemExit");
impl_exception_stub_type!(PyTimeoutError, "TimeoutError");
impl_exception_stub_type!(PyTypeError, "TypeError");
impl_exception_stub_type!(PyUnboundLocalError, "UnboundLocalError");
impl_exception_stub_type!(PyUnicodeDecodeError, "UnicodeDecodeError");
impl_exception_stub_type!(PyUnicodeEncodeError, "UnicodeEncodeError");
impl_exception_stub_type!(PyUnicodeError, "UnicodeError");
impl_exception_stub_type!(PyUnicodeTranslateError, "UnicodeTranslateError");
impl_exception_stub_type!(PyUnicodeWarning, "UnicodeWarning");
impl_exception_stub_type!(PyUserWarning, "UserWarning");
impl_exception_stub_type!(PyValueError, "ValueError");
impl_exception_stub_type!(PyWarning, "Warning");
impl_exception_stub_type!(PyZeroDivisionError, "ZeroDivisionError");
