/*
 * Copyright (C) 2016 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// Reference-only excerpt: generateStaticStubMethodSource from generateCpp.cpp.
void AST::generateStaticStubMethodSource(Formatter& out, const FQName& fqName,
                                         const Method* method, const Interface* superInterface) const {
    if (method->isHidlReserved() && method->overridesCppImpl(IMPL_STUB)) {
        return;
    }

    const std::string& klassName = fqName.getInterfaceStubName();

    out << "::android::status_t " << klassName << "::_hidl_" << method->name() << "(\n";

    out.indent();
    out.indent();

    out << "::android::hidl::base::V1_0::BnHwBase* _hidl_this,\n"
        << "const ::android::hardware::Parcel &_hidl_data,\n"
        << "::android::hardware::Parcel *_hidl_reply,\n"
        << "TransactCallback _hidl_cb) {\n";

    out.unindent();

    out << "#ifdef __ANDROID_DEBUGGABLE__\n";
    out << "bool mEnableInstrumentation = _hidl_this->isInstrumentationEnabled();\n";
    out << "const auto &mInstrumentationCallbacks = _hidl_this->getInstrumentationCallbacks();\n";
    out << "#endif // __ANDROID_DEBUGGABLE__\n\n";

    out << "::android::status_t _hidl_err = ::android::OK;\n";

    out << "if (!_hidl_data.enforceInterface("
        << klassName
        << "::Pure::descriptor)) {\n";

    out.indent();
    out << "_hidl_err = ::android::BAD_TYPE;\n";
    out << "return _hidl_err;\n";
    out.unindent();
    out << "}\n\n";

    declareCppReaderLocals(out, method->args(), false /* forResults */);

    for (const auto &arg : method->args()) {
        emitCppReaderWriter(
                out,
                "_hidl_data",
                false /* parcelObjIsPointer */,
                arg,
                true /* reader */,
                Type::ErrorMode_Return,
                false /* addPrefixToName */);
    }

    generateCppInstrumentationCall(
            out,
            InstrumentationEvent::SERVER_API_ENTRY,
            method,
            superInterface);

    const bool returnsValue = !method->results().empty();
    const NamedReference<Type>* elidedReturn = method->canElideCallback();

    std::string callee;

    if (method->isHidlReserved() && method->overridesCppImpl(IMPL_STUB_IMPL)) {
        callee = "_hidl_this";
    } else {
        callee = "static_cast<" + fqName.getInterfaceName() + "*>(_hidl_this->getImpl().get())";
    }

    if (elidedReturn != nullptr) {
        out << elidedReturn->type().getCppResultType()
            << " _hidl_out_"
            << elidedReturn->name()
            << " = "
            << callee << "->" << method->name()
            << "(";

        out.join(method->args().begin(), method->args().end(), ", ", [&] (const auto &arg) {
            if (arg->type().resultNeedsDeref()) {
                out << "*";
            }
            out << arg->name();
        });

        out << ");\n\n";

        out << "::android::hardware::writeToParcel(::android::hardware::Status::ok(), "
            << "_hidl_reply);\n\n";

        elidedReturn->type().emitReaderWriter(
                out,
                "_hidl_out_" + elidedReturn->name(),
                "_hidl_reply",
                true, /* parcelObjIsPointer */
                false, /* isReader */
                Type::ErrorMode_Goto);

        out.unindent();
        out << "_hidl_error:\n";
        out.indent();

        generateCppInstrumentationCall(
                out,
                InstrumentationEvent::SERVER_API_EXIT,
                method,
            superInterface);

        out << "if (_hidl_err != ::android::OK) { return _hidl_err; }\n";
        out << "_hidl_cb(*_hidl_reply);\n";
    } else {
        if (returnsValue) {
            out << "bool _hidl_callbackCalled = false;\n\n";
        }

        out << "::android::hardware::Return<void> _hidl_ret = " << callee << "->" << method->name()
            << "(";

        out.join(method->args().begin(), method->args().end(), ", ", [&] (const auto &arg) {
            if (arg->type().resultNeedsDeref()) {
                out << "*";
            }

            out << arg->name();
        });

        if (returnsValue) {
            if (!method->args().empty()) {
                out << ", ";
            }

            out << "[&](";

            out.join(method->results().begin(), method->results().end(), ", ", [&](const auto &arg) {
                out << "const auto &_hidl_out_" << arg->name();
            });

            out << ") {\n";
            out.indent();
            out << "if (_hidl_callbackCalled) {\n";
            out.indent();
            out << "LOG_ALWAYS_FATAL(\""
                << method->name()
                << ": _hidl_cb called a second time, but must be called once.\");\n";
            out.unindent();
            out << "}\n";
            out << "_hidl_callbackCalled = true;\n\n";

            out << "::android::hardware::writeToParcel(::android::hardware::Status::ok(), "
                << "_hidl_reply);\n\n";

            for (const auto &arg : method->results()) {
                emitCppReaderWriter(
                        out,
                        "_hidl_reply",
                        true /* parcelObjIsPointer */,
                        arg,
                        false /* reader */,
                        Type::ErrorMode_Goto,
                        true /* addPrefixToName */);
            }

            if (!method->results().empty()) {
                out.unindent();
                out << "_hidl_error:\n";
                out.indent();
            }

            generateCppInstrumentationCall(
                    out,
                    InstrumentationEvent::SERVER_API_EXIT,
                    method,
                    superInterface);

            out << "if (_hidl_err != ::android::OK) { return; }\n";
            out << "_hidl_cb(*_hidl_reply);\n";

            out.unindent();
            out << "});\n\n";
        } else {
            out << ");\n\n";
            out << "(void) _hidl_cb;\n\n";
            generateCppInstrumentationCall(
                    out,
                    InstrumentationEvent::SERVER_API_EXIT,
                    method,
                    superInterface);
        }

        out << "_hidl_ret.assertOk();\n";

        if (returnsValue) {
            out << "if (!_hidl_callbackCalled) {\n";
            out.indent();
            out << "LOG_ALWAYS_FATAL(\""
                << method->name()
                << ": _hidl_cb not called, but must be called once.\");\n";
            out.unindent();
            out << "}\n\n";
        } else {
            out << "::android::hardware::writeToParcel("
                << "::android::hardware::Status::ok(), "
                << "_hidl_reply);\n\n";
        }
    }

    out << "return _hidl_err;\n";
    out.unindent();
    out << "}\n\n";
}

