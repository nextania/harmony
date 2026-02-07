use anyhow::{Context, Result};
use openmls::{group::{GroupEpoch, GroupId}, prelude::{BasicCredential, CredentialWithKey, ExternalProposal, KeyPackage, KeyPackageBundle, KeyPackageIn, LeafNodeIndex, ProtocolVersion, SenderExtensionIndex}};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use tls_codec::{Deserialize, Serialize};


pub struct ExternalSenderIdentity {
    package: KeyPackageBundle,
    signer: SignatureKeyPair,
    provider: OpenMlsRustCrypto,
}

impl ExternalSenderIdentity {
    pub fn generate(server_id: &str) -> Result<Self> {
        let provider = OpenMlsRustCrypto::default();
        let ciphersuite = openmls::prelude::Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519;
        let credential = BasicCredential::new(server_id.into());
        let signature_keys =
            SignatureKeyPair::new(ciphersuite.signature_algorithm())
                .expect("Error generating a signature key pair.");
        signature_keys
            .store(provider.storage())
            .expect("Error storing signature keys in key store.");
        let package = KeyPackage::builder()
            .build(
                ciphersuite,
                &provider,
                &signature_keys,
                CredentialWithKey {
                    credential: credential.into(),
                    signature_key: signature_keys.public().into(),
                },
            )
            .unwrap();
        
        Ok(Self {
            package,
            signer: signature_keys,
            provider,
        })
    }
    
    pub fn serialize_credential(&self) -> Result<Vec<u8>> {
        Ok(self.package.key_package().tls_serialize_detached()?)
    }

    pub fn signature_public_key(&self) -> Vec<u8> {
        self.signer.public().to_vec()
    }
    
    pub fn create_add_proposal(
        &self,
        group_id: &[u8],
        epoch: u64,
        key_package_bytes: &[u8],
    ) -> Result<Vec<u8>> {
        let group_id = GroupId::from_slice(group_id);
        let epoch = GroupEpoch::from(epoch);
        let key_package_bytes = key_package_bytes.to_vec();
        let key_package_in = KeyPackageIn::tls_deserialize(&mut key_package_bytes.as_slice())
            .context("Failed to deserialize KeyPackage")?;
        let key_package = key_package_in.validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .context("Failed to validate KeyPackage")?;
        let proposal = ExternalProposal::new_add::<OpenMlsRustCrypto>(key_package, group_id, epoch, &self.signer, SenderExtensionIndex::new(0))?;
        Ok(proposal.to_bytes()?)
    }
    
    pub fn create_remove_proposal(
        &self,
        group_id: &[u8],
        epoch: u64,
        removed_index: u32,
    ) -> Result<Vec<u8>> {
        let group_id = GroupId::from_slice(group_id);
        let epoch = GroupEpoch::from(epoch);
        let idx = LeafNodeIndex::new(removed_index);
        let proposal = ExternalProposal::new_remove::<OpenMlsRustCrypto>(idx, group_id, epoch, &self.signer, SenderExtensionIndex::new(0))?;
        Ok(proposal.to_bytes()?)
    }
}
