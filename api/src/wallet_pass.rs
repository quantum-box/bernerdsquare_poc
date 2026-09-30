use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use cms::{
    cert::{CertificateChoices, IssuerAndSerialNumber},
    content_info::{CmsVersion, ContentInfo},
    signed_data::{
        CertificateSet, DigestAlgorithmIdentifiers, EncapsulatedContentInfo, SignatureValue,
        SignedAttributes, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
    },
};
use const_oid::ObjectIdentifier;
use der::{
    asn1::{
        Any, AnyRef, GeneralizedTime, Ia5StringRef, OctetString, PrintableStringRef, SetOfVec,
        UtcTime, Utf8StringRef,
    },
    Decode, Encode,
};
use serde::Serialize;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use spki::AlgorithmIdentifierOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use worker::Env;
use x509_cert::{
    attr::Attribute,
    ext::pkix::{ExtendedKeyUsage, KeyUsage},
    Certificate,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const PASS_TYPE_IDENTIFIER: &str = "WALLET_PASS_TYPE_IDENTIFIER";
const TEAM_IDENTIFIER: &str = "WALLET_TEAM_IDENTIFIER";
const ORGANIZATION_NAME: &str = "WALLET_ORGANIZATION_NAME";
const SIGNER_PRIVATE_KEY: &str = "WALLET_SIGNER_PRIVATE_KEY_PKCS8_B64";
const SIGNER_CERTIFICATE: &str = "WALLET_SIGNER_CERTIFICATE_DER_B64";
const WWDR_CERTIFICATE: &str = "WALLET_WWDR_CERTIFICATE_DER_B64";
// SHA-256 fingerprint of Apple's WWDR G4 certificate used by this Pass Type ID.
const APPLE_WWDR_G4_SHA256: &str =
    "ea4757885538dd8cb59ff4556f676087d83c85e70902c122e42c0808b5bce14c";

pub async fn configured(env: &Env) -> bool {
    let Ok(config) = PassConfig::from_env(env) else {
        return false;
    };
    validate_pass_configuration(&config).await.is_ok()
}

struct PassConfig {
    pass_type_identifier: String,
    team_identifier: String,
    organization_name: String,
    private_key_pkcs8: Vec<u8>,
    signer_certificate_der: Vec<u8>,
    wwdr_certificate_der: Vec<u8>,
}

impl PassConfig {
    fn from_env(env: &Env) -> Result<Self, ()> {
        let pass_type_identifier = required_var(env, PASS_TYPE_IDENTIFIER)?;
        let team_identifier = required_var(env, TEAM_IDENTIFIER)?;
        let organization_name = required_var(env, ORGANIZATION_NAME)?;
        if !pass_type_identifier.starts_with("pass.")
            || !pass_type_identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            || team_identifier.len() != 10
            || !team_identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            || organization_name.len() > 128
        {
            return Err(());
        }

        Ok(Self {
            pass_type_identifier,
            team_identifier,
            organization_name,
            private_key_pkcs8: required_secret_b64(env, SIGNER_PRIVATE_KEY)?,
            signer_certificate_der: required_secret_b64(env, SIGNER_CERTIFICATE)?,
            wwdr_certificate_der: required_secret_b64(env, WWDR_CERTIFICATE)?,
        })
    }
}

fn required_var(env: &Env, name: &str) -> Result<String, ()> {
    let value = env.var(name).map_err(|_| ())?.to_string();
    let value = value.trim().to_owned();
    if value.is_empty() {
        Err(())
    } else {
        Ok(value)
    }
}

fn required_secret_b64(env: &Env, name: &str) -> Result<Vec<u8>, ()> {
    let value = env.secret(name).map_err(|_| ())?.to_string();
    STANDARD.decode(value.trim()).map_err(|_| ())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletPass {
    format_version: u8,
    pass_type_identifier: String,
    serial_number: String,
    team_identifier: String,
    organization_name: String,
    description: String,
    logo_text: String,
    foreground_color: String,
    background_color: String,
    label_color: String,
    barcode: Barcode,
    barcodes: Vec<Barcode>,
    generic: GenericFields,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Barcode {
    format: &'static str,
    message: String,
    message_encoding: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenericFields {
    primary_fields: Vec<PassField>,
    secondary_fields: Vec<PassField>,
    back_fields: Vec<PassField>,
}

#[derive(Serialize)]
struct PassField {
    key: &'static str,
    label: &'static str,
    value: String,
}

pub async fn build_pkpass(env: &Env, credential_id: &str) -> Result<Vec<u8>, ()> {
    let config = PassConfig::from_env(env)?;
    validate_pass_configuration(&config).await?;
    if credential_id.is_empty()
        || credential_id.len() > 128
        || !credential_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(());
    }

    let barcode = Barcode {
        format: "PKBarcodeFormatQR",
        message: format!("BSQ-TEST-{credential_id}"),
        message_encoding: "iso-8859-1",
    };
    let pass = WalletPass {
        format_version: 1,
        pass_type_identifier: config.pass_type_identifier.clone(),
        serial_number: credential_id.to_owned(),
        team_identifier: config.team_identifier,
        organization_name: config.organization_name,
        description: "Banasku Touch Entry のWallet発行テスト用会員証".to_owned(),
        logo_text: "Banasku".to_owned(),
        foreground_color: "rgb(255, 255, 255)".to_owned(),
        background_color: "rgb(28, 43, 76)".to_owned(),
        label_color: "rgb(192, 207, 232)".to_owned(),
        barcode: barcode.clone(),
        barcodes: vec![barcode],
        generic: GenericFields {
            primary_fields: vec![PassField {
                key: "member",
                label: "種別",
                value: "テスト会員".to_owned(),
            }],
            secondary_fields: vec![PassField {
                key: "credential",
                label: "カードID",
                value: credential_id
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect(),
            }],
            back_fields: vec![PassField {
                key: "testNotice",
                label: "用途",
                value: "Wallet発行テスト用です。入場ゲート、NFC、物理解錠には連携していません。"
                    .to_owned(),
            }],
        },
    };

    let pass_json = serde_json::to_vec(&pass).map_err(|_| ())?;
    let mut files = BTreeMap::new();
    files.insert("pass.json".to_owned(), pass_json);
    files.insert(
        "icon.png".to_owned(),
        include_bytes!("../assets/wallet/icon.png").to_vec(),
    );
    files.insert(
        "icon@2x.png".to_owned(),
        include_bytes!("../assets/wallet/icon@2x.png").to_vec(),
    );

    let manifest = build_manifest(&files)?;
    let signature = sign_manifest(
        &manifest,
        &config.private_key_pkcs8,
        &config.signer_certificate_der,
        &config.wwdr_certificate_der,
    )
    .await?;
    build_archive(files, manifest, &signature)
}

fn build_manifest(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, ()> {
    let entries = files
        .iter()
        .map(|(name, contents)| (name.clone(), hex::encode(Sha1::digest(contents))))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_vec(&entries).map_err(|_| ())
}

fn build_archive(
    files: BTreeMap<String, Vec<u8>>,
    manifest: Vec<u8>,
    signature: &[u8],
) -> Result<Vec<u8>, ()> {
    let mut files = files;
    files.insert("manifest.json".to_owned(), manifest);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, contents) in files {
        writer.start_file(name, options).map_err(|_| ())?;
        writer.write_all(&contents).map_err(|_| ())?;
    }
    writer.start_file("signature", options).map_err(|_| ())?;
    writer.write_all(signature).map_err(|_| ())?;
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|_| ())
}

fn signed_attributes_signature_input(signed_attributes: &SignedAttributes) -> Result<Vec<u8>, ()> {
    signed_attributes.to_der().map_err(|_| ())
}

fn build_signed_attributes(manifest: &[u8]) -> Result<SignedAttributes, ()> {
    let content_type_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3");
    let message_digest_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
    let signing_time_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.5");
    let data_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
    let content_type_value = Any::encode_from(&data_oid).map_err(|_| ())?;
    let digest = Sha1::digest(manifest);
    let digest_value =
        Any::encode_from(&OctetString::new(digest.to_vec()).map_err(|_| ())?).map_err(|_| ())?;
    let signing_time_value = encode_signing_time(current_signing_time()?)?;

    SignedAttributes::try_from(vec![
        Attribute {
            oid: content_type_oid,
            values: SetOfVec::try_from(vec![content_type_value]).map_err(|_| ())?,
        },
        Attribute {
            oid: message_digest_oid,
            values: SetOfVec::try_from(vec![digest_value]).map_err(|_| ())?,
        },
        Attribute {
            oid: signing_time_oid,
            values: SetOfVec::try_from(vec![signing_time_value]).map_err(|_| ())?,
        },
    ])
    .map_err(|_| ())
}

fn current_signing_time() -> Result<SystemTime, ()> {
    #[cfg(target_arch = "wasm32")]
    {
        let now_millis = js_sys::Date::now();
        if !now_millis.is_finite() || now_millis < 0.0 {
            return Err(());
        }
        UNIX_EPOCH
            .checked_add(Duration::from_millis(now_millis as u64))
            .ok_or(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(SystemTime::now())
    }
}

fn encode_signing_time(time: SystemTime) -> Result<Any, ()> {
    if let Ok(utc_time) = UtcTime::from_system_time(time) {
        return Any::encode_from(&utc_time).map_err(|_| ());
    }

    let generalized_time = GeneralizedTime::from_system_time(time).map_err(|_| ())?;
    Any::encode_from(&generalized_time).map_err(|_| ())
}

fn prepare_manifest_signature(manifest: &[u8]) -> Result<(SignedAttributes, Vec<u8>), ()> {
    let signed_attributes = build_signed_attributes(manifest)?;
    let bytes_to_sign = signed_attributes_signature_input(&signed_attributes)?;
    Ok((signed_attributes, bytes_to_sign))
}

fn build_signed_cms(
    signed_attributes: SignedAttributes,
    signature: &[u8],
    signer_certificate_der: &[u8],
    wwdr_certificate_der: &[u8],
) -> Result<Vec<u8>, ()> {
    if signature.is_empty() {
        return Err(());
    }
    let signer_certificate = Certificate::from_der(signer_certificate_der).map_err(|_| ())?;
    let wwdr_certificate = Certificate::from_der(wwdr_certificate_der).map_err(|_| ())?;
    let sha1_oid = ObjectIdentifier::new_unwrap("1.3.14.3.2.26");
    let rsa_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
    let data_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
    let digest_algorithm = AlgorithmIdentifierOwned {
        oid: sha1_oid,
        parameters: Some(Any::null()),
    };
    let signature_algorithm = AlgorithmIdentifierOwned {
        oid: rsa_oid,
        parameters: Some(Any::null()),
    };
    let signer_info = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: signer_certificate.tbs_certificate.issuer.clone(),
            serial_number: signer_certificate.tbs_certificate.serial_number.clone(),
        }),
        digest_alg: digest_algorithm.clone(),
        signed_attrs: Some(signed_attributes),
        signature_algorithm,
        signature: SignatureValue::new(signature.to_vec()).map_err(|_| ())?,
        unsigned_attrs: None,
    };
    let certificates = CertificateSet::from(
        SetOfVec::try_from(vec![
            CertificateChoices::Certificate(signer_certificate),
            CertificateChoices::Certificate(wwdr_certificate),
        ])
        .map_err(|_| ())?,
    );
    let signed_data = SignedData {
        version: CmsVersion::V1,
        digest_algorithms: DigestAlgorithmIdentifiers::try_from(vec![digest_algorithm])
            .map_err(|_| ())?,
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: data_oid,
            econtent: None,
        },
        certificates: Some(certificates),
        crls: None,
        signer_infos: SignerInfos::from(SetOfVec::try_from(vec![signer_info]).map_err(|_| ())?),
    };
    let signed_data_der = signed_data.to_der().map_err(|_| ())?;
    let signed_data_any = Any::from(AnyRef::try_from(signed_data_der.as_slice()).map_err(|_| ())?);
    ContentInfo {
        content_type: ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2"),
        content: signed_data_any,
    }
    .to_der()
    .map_err(|_| ())
}

async fn sign_manifest(
    manifest: &[u8],
    private_key_pkcs8: &[u8],
    signer_certificate_der: &[u8],
    wwdr_certificate_der: &[u8],
) -> Result<Vec<u8>, ()> {
    let (signed_attributes, bytes_to_sign) = prepare_manifest_signature(manifest)?;
    let signature = web_crypto_rsa_sha1_sign(private_key_pkcs8, &bytes_to_sign).await?;
    build_signed_cms(
        signed_attributes,
        &signature,
        signer_certificate_der,
        wwdr_certificate_der,
    )
}

async fn validate_pass_configuration(config: &PassConfig) -> Result<(), ()> {
    let signer_certificate =
        Certificate::from_der(&config.signer_certificate_der).map_err(|_| ())?;
    let wwdr_certificate = Certificate::from_der(&config.wwdr_certificate_der).map_err(|_| ())?;
    if !signer_identity_matches(
        &signer_certificate,
        &config.pass_type_identifier,
        &config.team_identifier,
    ) || !has_pass_type_signing_usage(&signer_certificate)
        || !is_apple_wwdr_g4(&wwdr_certificate)
        || hex::encode(Sha256::digest(&config.wwdr_certificate_der)) != APPLE_WWDR_G4_SHA256
        || signer_certificate.tbs_certificate.issuer != wwdr_certificate.tbs_certificate.subject
        || signer_certificate.signature_algorithm != signer_certificate.tbs_certificate.signature
        || !is_rsa_certificate(&signer_certificate)
        || !is_rsa_certificate(&wwdr_certificate)
    {
        return Err(());
    }

    let now_millis = js_sys::Date::now();
    if !now_millis.is_finite() || now_millis < 0.0 {
        return Err(());
    }
    let now = UNIX_EPOCH
        .checked_add(Duration::from_millis(now_millis as u64))
        .ok_or(())?;
    if !certificate_is_current(&signer_certificate, now)
        || !certificate_is_current(&wwdr_certificate, now)
    {
        return Err(());
    }

    let signer_public_key = signer_certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|_| ())?;
    let wwdr_public_key = wwdr_certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|_| ())?;
    let signature_algorithm_oid = signer_certificate.signature_algorithm.oid.to_string();
    let certificate_signature_hash = match signature_algorithm_oid.as_str() {
        "1.2.840.113549.1.1.11" => "SHA-256",
        "1.2.840.113549.1.1.5" => "SHA-1",
        _ => return Err(()),
    };
    let signer_tbs = signer_certificate
        .tbs_certificate
        .to_der()
        .map_err(|_| ())?;
    let issuer_signature = signer_certificate.signature.as_bytes().ok_or(())?;
    if !web_crypto_rsa_verify(
        &wwdr_public_key,
        certificate_signature_hash,
        &signer_tbs,
        issuer_signature,
    )
    .await?
    {
        return Err(());
    }

    let challenge = b"banasku-wallet-signing-key-match";
    let key_check_signature =
        web_crypto_rsa_sign(&config.private_key_pkcs8, "SHA-256", challenge).await?;
    if !web_crypto_rsa_verify(
        &signer_public_key,
        "SHA-256",
        challenge,
        &key_check_signature,
    )
    .await?
    {
        return Err(());
    }
    Ok(())
}

fn signer_identity_matches(certificate: &Certificate, pass_type_id: &str, team_id: &str) -> bool {
    subject_attribute(
        &certificate.tbs_certificate.subject,
        "0.9.2342.19200300.100.1.1",
    )
    .is_ok_and(|value| value == pass_type_id)
        && subject_attribute(&certificate.tbs_certificate.subject, "2.5.4.11")
            .is_ok_and(|value| value == team_id)
}

fn has_pass_type_signing_usage(certificate: &Certificate) -> bool {
    const PASS_TYPE_ID_EXTENDED_KEY_USAGE: &str = "1.2.840.113635.100.4.14";
    let Some(extensions) = certificate.tbs_certificate.extensions.as_ref() else {
        return false;
    };
    let key_usage_oid = ObjectIdentifier::new_unwrap("2.5.29.15");
    let extended_key_usage_oid = ObjectIdentifier::new_unwrap("2.5.29.37");
    let key_usage = extensions
        .iter()
        .find(|extension| extension.extn_id == key_usage_oid)
        .and_then(|extension| KeyUsage::from_der(extension.extn_value.as_bytes()).ok());
    let extended_key_usage = extensions
        .iter()
        .find(|extension| extension.extn_id == extended_key_usage_oid)
        .and_then(|extension| ExtendedKeyUsage::from_der(extension.extn_value.as_bytes()).ok());

    key_usage.is_some_and(|usage| usage.digital_signature())
        && extended_key_usage.is_some_and(|usage| {
            usage
                .0
                .iter()
                .any(|oid| oid.to_string() == PASS_TYPE_ID_EXTENDED_KEY_USAGE)
        })
}

fn is_apple_wwdr_g4(certificate: &Certificate) -> bool {
    subject_attribute(&certificate.tbs_certificate.subject, "2.5.4.3")
        .is_ok_and(|value| value == "Apple Worldwide Developer Relations Certification Authority")
        && subject_attribute(&certificate.tbs_certificate.subject, "2.5.4.11")
            .is_ok_and(|value| value == "G4")
        && subject_attribute(&certificate.tbs_certificate.subject, "2.5.4.10")
            .is_ok_and(|value| value == "Apple Inc.")
        && subject_attribute(&certificate.tbs_certificate.subject, "2.5.4.6")
            .is_ok_and(|value| value == "US")
}

fn subject_attribute(subject: &x509_cert::name::Name, oid: &str) -> Result<String, ()> {
    let mut attributes = subject
        .0
        .iter()
        .flat_map(|rdn| rdn.0.iter())
        .filter(|attribute| attribute.oid.to_string() == oid);
    let attribute = attributes.next().ok_or(())?;
    if attributes.next().is_some() {
        return Err(());
    }
    let value = &attribute.value;
    PrintableStringRef::try_from(value)
        .map(|value| value.as_str().to_owned())
        .or_else(|_| Utf8StringRef::try_from(value).map(|value| value.as_str().to_owned()))
        .or_else(|_| Ia5StringRef::try_from(value).map(|value| value.as_str().to_owned()))
        .map_err(|_| ())
}

fn is_rsa_certificate(certificate: &Certificate) -> bool {
    certificate
        .tbs_certificate
        .subject_public_key_info
        .algorithm
        .oid
        .to_string()
        == "1.2.840.113549.1.1.1"
}

fn certificate_is_current(certificate: &Certificate, now: SystemTime) -> bool {
    let validity = certificate.tbs_certificate.validity;
    now >= validity.not_before.to_system_time() && now < validity.not_after.to_system_time()
}

async fn web_crypto_rsa_sha1_sign(private_key_pkcs8: &[u8], message: &[u8]) -> Result<Vec<u8>, ()> {
    web_crypto_rsa_sign(private_key_pkcs8, "SHA-1", message).await
}

async fn web_crypto_rsa_sign(
    private_key_pkcs8: &[u8],
    hash_name: &str,
    message: &[u8],
) -> Result<Vec<u8>, ()> {
    let global = js_sys::global();
    let crypto = js_sys::Reflect::get(&global, &JsValue::from_str("crypto")).map_err(|_| ())?;
    let subtle = js_sys::Reflect::get(&crypto, &JsValue::from_str("subtle")).map_err(|_| ())?;
    let import_key = js_sys::Reflect::get(&subtle, &JsValue::from_str("importKey"))
        .map_err(|_| ())?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| ())?;
    let sign = js_sys::Reflect::get(&subtle, &JsValue::from_str("sign"))
        .map_err(|_| ())?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| ())?;

    let import_algorithm = js_sys::Object::new();
    set_property(
        &import_algorithm,
        "name",
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )?;
    let hash = js_sys::Object::new();
    set_property(&hash, "name", &JsValue::from_str(hash_name))?;
    set_property(&import_algorithm, "hash", &hash.into())?;
    let usages = js_sys::Array::new();
    usages.push(&JsValue::from_str("sign"));
    let import_arguments = js_sys::Array::new();
    import_arguments.push(&JsValue::from_str("pkcs8"));
    import_arguments.push(&js_sys::Uint8Array::from(private_key_pkcs8).into());
    import_arguments.push(&import_algorithm.into());
    import_arguments.push(&JsValue::FALSE);
    import_arguments.push(&usages.into());
    let import_promise = import_key
        .apply(&subtle, &import_arguments)
        .map_err(|_| ())?;
    let crypto_key = JsFuture::from(js_sys::Promise::resolve(&import_promise))
        .await
        .map_err(|_| ())?;

    let sign_algorithm = js_sys::Object::new();
    set_property(
        &sign_algorithm,
        "name",
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )?;
    let sign_arguments = js_sys::Array::new();
    sign_arguments.push(&sign_algorithm.into());
    sign_arguments.push(&crypto_key);
    sign_arguments.push(&js_sys::Uint8Array::from(message).into());
    let sign_promise = sign.apply(&subtle, &sign_arguments).map_err(|_| ())?;
    let signature = JsFuture::from(js_sys::Promise::resolve(&sign_promise))
        .await
        .map_err(|_| ())?;
    let bytes = js_sys::Uint8Array::new(&signature);
    let mut output = vec![0; bytes.length() as usize];
    bytes.copy_to(&mut output);
    if output.is_empty() {
        Err(())
    } else {
        Ok(output)
    }
}

async fn web_crypto_rsa_verify(
    public_key_spki: &[u8],
    hash_name: &str,
    message: &[u8],
    signature: &[u8],
) -> Result<bool, ()> {
    let global = js_sys::global();
    let crypto = js_sys::Reflect::get(&global, &JsValue::from_str("crypto")).map_err(|_| ())?;
    let subtle = js_sys::Reflect::get(&crypto, &JsValue::from_str("subtle")).map_err(|_| ())?;
    let import_key = js_sys::Reflect::get(&subtle, &JsValue::from_str("importKey"))
        .map_err(|_| ())?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| ())?;
    let verify = js_sys::Reflect::get(&subtle, &JsValue::from_str("verify"))
        .map_err(|_| ())?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| ())?;

    let import_algorithm = js_sys::Object::new();
    set_property(
        &import_algorithm,
        "name",
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )?;
    let hash = js_sys::Object::new();
    set_property(&hash, "name", &JsValue::from_str(hash_name))?;
    set_property(&import_algorithm, "hash", &hash.into())?;
    let usages = js_sys::Array::new();
    usages.push(&JsValue::from_str("verify"));
    let import_arguments = js_sys::Array::new();
    import_arguments.push(&JsValue::from_str("spki"));
    import_arguments.push(&js_sys::Uint8Array::from(public_key_spki).into());
    import_arguments.push(&import_algorithm.into());
    import_arguments.push(&JsValue::FALSE);
    import_arguments.push(&usages.into());
    let import_promise = import_key
        .apply(&subtle, &import_arguments)
        .map_err(|_| ())?;
    let public_key = JsFuture::from(js_sys::Promise::resolve(&import_promise))
        .await
        .map_err(|_| ())?;

    let verify_algorithm = js_sys::Object::new();
    set_property(
        &verify_algorithm,
        "name",
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )?;
    let verify_arguments = js_sys::Array::new();
    verify_arguments.push(&verify_algorithm.into());
    verify_arguments.push(&public_key);
    verify_arguments.push(&js_sys::Uint8Array::from(signature).into());
    verify_arguments.push(&js_sys::Uint8Array::from(message).into());
    let verify_promise = verify.apply(&subtle, &verify_arguments).map_err(|_| ())?;
    let verified = JsFuture::from(js_sys::Promise::resolve(&verify_promise))
        .await
        .map_err(|_| ())?;
    verified.as_bool().ok_or(())
}

fn set_property(object: &js_sys::Object, key: &str, value: &JsValue) -> Result<(), ()> {
    js_sys::Reflect::set(object, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{
        build_archive, build_manifest, build_signed_attributes, build_signed_cms,
        has_pass_type_signing_usage, is_apple_wwdr_g4, prepare_manifest_signature,
        signed_attributes_signature_input, signer_identity_matches,
    };
    use cms::{
        cert::CertificateChoices,
        content_info::ContentInfo,
        signed_data::{SignedAttributes, SignedData, SignerIdentifier},
    };
    use const_oid::ObjectIdentifier;
    use der::{
        asn1::{Any, AnyRef, GeneralizedTime, OctetString, OctetStringRef, SetOfVec},
        Decode, Encode,
    };
    use rsa::{
        pkcs1v15::{Signature, SigningKey, VerifyingKey},
        pkcs8::{DecodePrivateKey, DecodePublicKey},
        signature::{SignatureEncoding, Signer, Verifier},
        RsaPublicKey,
    };
    use sha1::{Digest, Sha1};
    use std::{
        collections::BTreeMap,
        io::{Cursor, Read},
    };
    use x509_cert::attr::Attribute;
    use x509_cert::Certificate;
    use zip::ZipArchive;

    const TEST_SIGNER_KEY: &[u8] = include_bytes!("../tests/fixtures/wallet/test-signer-key.pk8");
    const TEST_SIGNER_CERTIFICATE: &[u8] =
        include_bytes!("../tests/fixtures/wallet/test-signer.der");
    const TEST_WWDR_CERTIFICATE: &[u8] = include_bytes!("../tests/fixtures/wallet/test-wwdr.der");

    #[test]
    fn cms_signed_attributes_signature_input_uses_the_set_of_tag() {
        let data_oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
        let content_type = Attribute {
            oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3"),
            values: SetOfVec::try_from(vec![Any::encode_from(&data_oid).unwrap()]).unwrap(),
        };
        let message_digest = Attribute {
            oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4"),
            values: SetOfVec::try_from(vec![Any::encode_from(
                &OctetString::new(vec![0; 20]).unwrap(),
            )
            .unwrap()])
            .unwrap(),
        };
        let signed_attributes =
            SignedAttributes::try_from(vec![content_type, message_digest]).unwrap();

        let bytes = signed_attributes_signature_input(&signed_attributes).unwrap();

        assert_eq!(bytes.first(), Some(&0x31));
    }

    #[test]
    fn manifest_and_archive_include_only_hashed_pass_assets_and_detached_signature() {
        let files = BTreeMap::from([
            ("icon.png".to_owned(), b"icon".to_vec()),
            ("pass.json".to_owned(), b"pass".to_vec()),
        ]);
        let manifest = build_manifest(&files).unwrap();
        let manifest_text = String::from_utf8(manifest.clone()).unwrap();
        assert!(manifest_text.contains("icon.png"));
        assert!(manifest_text.contains("pass.json"));
        assert!(!manifest_text.contains("manifest.json"));

        let archive = build_archive(files, manifest, b"test-cms-signature").unwrap();
        let mut zip = ZipArchive::new(Cursor::new(archive)).unwrap();
        assert!(zip.by_name("pass.json").is_ok());
        assert!(zip.by_name("icon.png").is_ok());
        assert_eq!(
            zip.file_names().filter(|name| *name == "signature").count(),
            1
        );
        let mut archived_manifest = Vec::new();
        zip.by_name("manifest.json")
            .unwrap()
            .read_to_end(&mut archived_manifest)
            .unwrap();
        let archived_manifest: BTreeMap<String, String> =
            serde_json::from_slice(&archived_manifest).unwrap();
        assert_eq!(
            archived_manifest["pass.json"],
            hex::encode(Sha1::digest(b"pass"))
        );
        assert_eq!(
            archived_manifest["icon.png"],
            hex::encode(Sha1::digest(b"icon"))
        );
        let mut archived_signature = Vec::new();
        zip.by_name("signature")
            .unwrap()
            .read_to_end(&mut archived_signature)
            .unwrap();
        assert_eq!(archived_signature, b"test-cms-signature");
    }

    #[test]
    fn generated_detached_cms_signature_verifies_with_its_embedded_pass_certificate() {
        let manifest = br#"{"icon.png":"abc123","pass.json":"def456"}"#;
        let (signed_attributes, bytes_to_sign) = prepare_manifest_signature(manifest).unwrap();
        let private_key = rsa::RsaPrivateKey::from_pkcs8_der(TEST_SIGNER_KEY).unwrap();
        let signing_key = SigningKey::<Sha1>::new(private_key);
        let signature = signing_key.sign(&bytes_to_sign);
        let cms_der = build_signed_cms(
            signed_attributes.clone(),
            &signature.to_vec(),
            TEST_SIGNER_CERTIFICATE,
            TEST_WWDR_CERTIFICATE,
        )
        .unwrap();

        let content_info = ContentInfo::from_der(&cms_der).unwrap();
        assert_eq!(
            content_info.content_type,
            ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2")
        );
        let signed_data_der = content_info.content.to_der().unwrap();
        let signed_data = SignedData::from_der(&signed_data_der).unwrap();
        assert_eq!(signed_data.signer_infos.0.len(), 1);
        assert_eq!(signed_data.certificates.as_ref().unwrap().0.len(), 2);
        assert_eq!(
            signed_data.encap_content_info.econtent_type,
            ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1")
        );
        assert!(signed_data.encap_content_info.econtent.is_none());
        assert!(signed_data
            .digest_algorithms
            .iter()
            .any(|algorithm| algorithm.oid == ObjectIdentifier::new_unwrap("1.3.14.3.2.26")));

        let signer_info = signed_data.signer_infos.0.iter().next().unwrap();
        assert_eq!(
            signer_info.digest_alg.oid,
            ObjectIdentifier::new_unwrap("1.3.14.3.2.26")
        );
        assert_eq!(
            signer_info.signature_algorithm.oid,
            ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1")
        );
        let embedded_attributes = signer_info.signed_attrs.as_ref().unwrap();
        assert_eq!(embedded_attributes, &signed_attributes);
        assert_eq!(
            embedded_attributes.to_der().unwrap().first(),
            Some(&0x31),
            "CMS signs the DER SET OF encoding of signedAttrs"
        );
        let signing_time_attribute = embedded_attributes
            .iter()
            .find(|attribute| attribute.oid == ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.5"))
            .expect("CMS signature includes the signing-time attribute");
        assert_eq!(signing_time_attribute.values.len(), 1);
        let signing_time_value = AnyRef::from(signing_time_attribute.values.iter().next().unwrap());
        assert!(
            signing_time_value.decode_as::<der::asn1::UtcTime>().is_ok()
                || signing_time_value.decode_as::<GeneralizedTime>().is_ok()
        );
        let message_digest = embedded_attributes
            .iter()
            .find(|attribute| attribute.oid == ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4"))
            .unwrap()
            .values
            .iter()
            .next()
            .unwrap();
        let message_digest = AnyRef::from(message_digest)
            .decode_as::<OctetStringRef>()
            .unwrap();
        assert_eq!(message_digest.as_bytes(), Sha1::digest(manifest).as_slice());

        let signer_certificate = Certificate::from_der(TEST_SIGNER_CERTIFICATE).unwrap();
        match &signer_info.sid {
            SignerIdentifier::IssuerAndSerialNumber(signer_id) => {
                assert_eq!(signer_id.issuer, signer_certificate.tbs_certificate.issuer);
                assert_eq!(
                    signer_id.serial_number,
                    signer_certificate.tbs_certificate.serial_number
                );
            }
            SignerIdentifier::SubjectKeyIdentifier(_) => panic!("unexpected signer identifier"),
        }
        assert!(signed_data.certificates.as_ref().unwrap().0.iter().any(|choice| {
            matches!(choice, CertificateChoices::Certificate(certificate)
                if certificate.tbs_certificate.issuer == signer_certificate.tbs_certificate.issuer
                    && certificate.tbs_certificate.serial_number == signer_certificate.tbs_certificate.serial_number)
        }));
        let embedded_signature = Signature::try_from(signer_info.signature.as_bytes()).unwrap();
        let public_key_der = signer_certificate
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .unwrap();
        let public_key = RsaPublicKey::from_public_key_der(&public_key_der).unwrap();
        VerifyingKey::<Sha1>::new(public_key)
            .verify(&embedded_attributes.to_der().unwrap(), &embedded_signature)
            .expect("CMS signature must verify against the signer certificate");

        let changed_attributes = build_signed_attributes(b"changed manifest").unwrap();
        assert_ne!(embedded_attributes, &changed_attributes);
        assert!(VerifyingKey::<Sha1>::new(
            RsaPublicKey::from_public_key_der(
                &signer_certificate
                    .tbs_certificate
                    .subject_public_key_info
                    .to_der()
                    .unwrap()
            )
            .unwrap()
        )
        .verify(&changed_attributes.to_der().unwrap(), &embedded_signature)
        .is_err());
    }

    #[test]
    fn pass_certificate_subjects_must_match_expected_pass_and_team_ids() {
        let signer_certificate = Certificate::from_der(TEST_SIGNER_CERTIFICATE).unwrap();
        let wwdr_certificate = Certificate::from_der(TEST_WWDR_CERTIFICATE).unwrap();
        assert!(signer_identity_matches(
            &signer_certificate,
            "pass.jp.quantumbox.banasku.touch-entry-poc",
            "TESTTEAM01"
        ));
        assert!(has_pass_type_signing_usage(&signer_certificate));
        assert!(!signer_identity_matches(
            &signer_certificate,
            "pass.jp.quantumbox.other-pass",
            "TESTTEAM01"
        ));
        assert!(!signer_identity_matches(
            &signer_certificate,
            "pass.jp.quantumbox.banasku.touch-entry-poc",
            "OTHERTEAM1"
        ));
        assert!(is_apple_wwdr_g4(&wwdr_certificate));
    }

    #[test]
    fn cms_signing_time_uses_utc_time_through_2049_and_generalized_time_afterward() {
        let last_utc_time = der::DateTime::new(2049, 12, 31, 23, 59, 59)
            .unwrap()
            .to_system_time();
        let last_utc_value = super::encode_signing_time(last_utc_time).unwrap();
        assert!(AnyRef::from(&last_utc_value)
            .decode_as::<der::asn1::UtcTime>()
            .is_ok());

        let first_generalized_time = der::DateTime::new(2050, 1, 1, 0, 0, 0)
            .unwrap()
            .to_system_time();
        let first_generalized_value = super::encode_signing_time(first_generalized_time).unwrap();
        assert!(AnyRef::from(&first_generalized_value)
            .decode_as::<GeneralizedTime>()
            .is_ok());
    }
}
