# Retained AI Reference Code

`lib-ai` is retained temporarily as source material while the replacement AI
runtime and graph integration are designed.

It is intentionally inactive:

- it is excluded from the backend Cargo workspace;
- no active HeliOS crate depends on it;
- it is not built or tested by repository validation;
- it is not packaged by Gaia or installed in the device image;
- its former Daedalus plugin has been removed;
- its former `lib-cv` dependency no longer exists.

The code is not a supported library and is not expected to compile in its
current retained state. New product code must not depend on it. When the
replacement AI implementation is ready, useful backend/model-management ideas
may be migrated deliberately and this retained tree should then be deleted.
