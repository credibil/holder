//! Tests for wallet-initiated issuance flow where the authorization request is
//! made using a credential definition.
mod provider;

use credibil_holder::infosec::jose::jws::JwsBuilder;
use credibil_holder::issuance::proof::{self, Payload, Type, Verify};
use credibil_holder::issuance::{
    AuthCode, AuthorizationDetail, AuthorizationDetailType, CredentialAuthorization,
    CredentialResponseType, Format, IssuanceFlow, NotAccepted, ProfileClaims, WithoutOffer,
    WithoutToken,
};
use credibil_holder::provider::{Issuer, MetadataRequest, OAuthServerRequest};
use credibil_vc::test_utils::issuer::{
    self, CLIENT_ID, CREDENTIAL_ISSUER, NORMAL_USER, REDIRECT_URI,
};
use insta::assert_yaml_snapshot;

use crate::provider as holder;

// Test end-to-end wallet-initiated issuance flow, with authorization request
// using a credential definition.
#[tokio::test]
async fn wallet_credential_definition() {
    let issuer_provider = issuer::Provider::new();
    let provider = holder::Provider::new(Some(issuer_provider), None);

    //--------------------------------------------------------------------------
    // Get issuer metadata.
    //--------------------------------------------------------------------------
    let metadata_request = MetadataRequest {
        credential_issuer: CREDENTIAL_ISSUER.into(),
        languages: None,
    };
    let issuer_metadata =
        provider.metadata(metadata_request).await.expect("should get issuer metadata");

    //--------------------------------------------------------------------------
    // Get authorization server metadata.
    //--------------------------------------------------------------------------
    let auth_request = OAuthServerRequest {
        credential_issuer: CREDENTIAL_ISSUER.into(),
        issuer: None,
    };
    let auth_metadata =
        provider.oauth_server(auth_request).await.expect("should get auth metadata");

    //--------------------------------------------------------------------------
    // Initiate flow state with the offer and metadata.
    //--------------------------------------------------------------------------
    let state = IssuanceFlow::<WithoutOffer, AuthCode, NotAccepted, WithoutToken>::new(
        CLIENT_ID,
        NORMAL_USER,
        issuer_metadata.credential_issuer.clone(),
        auth_metadata.authorization_server,
    );

    //--------------------------------------------------------------------------
    // Construct an authorization request using the credential definition for
    // the employee ID credential. The expected user workflow would be to
    // present the issuer's metadata to the user and allow them to select which
    // credential and claims they want to request. Here we are hardcoding the
    // credential configuration ID for the employee ID credential and asking
    // for all claims in the credential.
    //--------------------------------------------------------------------------
    let cred_config = issuer_metadata
        .credential_issuer
        .credential_configurations_supported
        .get("EmployeeID_JWT")
        .expect("should have credential configuration");
    let claims = match &cred_config.format {
        Format::JwtVcJson(def) => ProfileClaims::W3c(def.credential_definition.clone()),
        _ => panic!("unexpected format"),
    };
    let accept = vec![AuthorizationDetail {
        type_: AuthorizationDetailType::OpenIdCredential,
        credential: CredentialAuthorization::ConfigurationId {
            credential_configuration_id: "EmployeeID_JWT".into(),
            claims: Some(claims),
        },
        locations: Some(vec![CREDENTIAL_ISSUER.into()]),
    }];
    let state = state.accept(accept);

    let (auth_request, verifier) = state
        .authorization_request(Some(REDIRECT_URI))
        .expect("should construct authorization request");
    let auth_response = provider.authorization(auth_request).await.expect("should authorize");

    //--------------------------------------------------------------------------
    // Exchange the authorization code for an access token.
    //--------------------------------------------------------------------------
    let token_request = state.token_request(&auth_response.code, &verifier, Some(REDIRECT_URI));
    let token_response = provider.token(token_request).await.expect("should get token response");
    let mut state = state.token(token_response.clone());

    //--------------------------------------------------------------------------
    // Make credential requests.
    //--------------------------------------------------------------------------
    // For this test we are going to accept all credentials on offer. (Just one
    // in this case but we demonstate the pattern for multiple credentials.) We
    // are making the request by credential identifier.
    let Some(authorized) = &token_response.authorization_details else {
        panic!("no authorization details in token response");
    };
    let mut identifiers = vec![];
    for auth in authorized {
        for id in auth.credential_identifiers.iter() {
            identifiers.push(id.clone());
        }
    }
    let jws_claims = state.proof();
    let jws = JwsBuilder::new()
        .jwt_type(Type::Openid4VciProofJwt)
        .payload(jws_claims)
        .add_signer(&provider)
        .build()
        .await
        .expect("should build jws");
    let jwt = jws.encode().expect("should encode jws");
    let credential_requests = state.credential_requests(&identifiers, &jwt).clone();
    for request in credential_requests {
        let credential_response =
            provider.credential(request.1).await.expect("should get credentials");
        // A credential response could contain a single credential, multiple
        // credentials or a deferred transaction ID. Any credential issued also
        // needs its proof verified by using a DID resolver.
        match credential_response.response {
            CredentialResponseType::Credential(vc_kind) => {
                // Single credential in response.
                let Payload::Vc { vc, issued_at } =
                    proof::verify(Verify::Vc(&vc_kind), provider.clone())
                        .await
                        .expect("should parse credential")
                else {
                    panic!("expected Payload::Vc");
                };
                state
                    .add_credential(&vc, &vc_kind, &issued_at, &request.0, None, None)
                    .expect("should add credential");
            }
            CredentialResponseType::Credentials(creds) => {
                // Multiple credentials in response.
                for vc_kind in creds {
                    let Payload::Vc { vc, issued_at } =
                        proof::verify(Verify::Vc(&vc_kind), provider.clone())
                            .await
                            .expect("should parse credential")
                    else {
                        panic!("expected Payload::Vc");
                    };
                    state
                        .add_credential(&vc, &vc_kind, &issued_at, &request.0, None, None)
                        .expect("should add credential");
                }
            }
            CredentialResponseType::TransactionId(tx_id) => {
                // Deferred transaction ID.
                state.add_deferred(&tx_id, &request.0);
            }
        }
    }

    // The flow is complete and the credential on the issuance state could now
    // be saved to the wallet using the `CredentialStorer` provider trait. For
    // the test we check snapshot of the state's credentials.
    assert_yaml_snapshot!("credentials", state.credentials(), {
        "[].type" => insta::sorted_redaction(),
        "[].subject_claims[]" => insta::sorted_redaction(),
        "[].subject_claims[].claims" => insta::sorted_redaction(),
        "[].subject_claims[].claims.address" => insta::sorted_redaction(),
        "[].claim_definitions" => insta::sorted_redaction(),
        "[].claim_definitions.address" => insta::sorted_redaction(),
        "[].issued" => "[issued]",
        "[].issuance_date" => "[issuance_date]",
    });
}
