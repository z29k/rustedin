<p align="center">
  <img src="assets/rustedin-logo.jpg" alt="logo rustedin" width="320">
</p>

<h1 align="center">rustedin</h1>

<p align="center">
  CLI multi-comptes pour LinkedIn — poste, partage et republie depuis des comptes personnels et des pages entreprise.
</p>

<p align="center">
  <a href="https://github.com/z29k/rustedin/actions/workflows/ci.yml"><img src="https://github.com/z29k/rustedin/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/z29k/rustedin/releases"><img src="https://img.shields.io/github/v/release/z29k/rustedin?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.74%2B-orange.svg" alt="Rust 1.74+">
</p>

<p align="center">
  <a href="README.md">English</a> · <b>Français</b>
</p>

---

## Table des matières

- [Fonctionnalités](#fonctionnalités)
- [Installation](#installation)
- [Démarrage rapide](#démarrage-rapide)
- [Référence des commandes](#référence-des-commandes)
  - [Option globale `--config`](#option-globale---config)
  - [`setup`](#setup)
  - [`auth`](#auth)
  - [`accounts`](#accounts)
  - [`status`](#status)
  - [`post`](#post)
  - [`reshare`](#reshare)
  - [`share`](#share)
  - [`get-post`](#get-post)
  - [`comments`](#comments)
  - [`profile`](#profile)
- [Visibilité (`--visibility`)](#visibilité---visibility)
- [Fichier de configuration (`rustedin.json`)](#fichier-de-configuration-rustedinjson)
- [Gestion des tokens](#gestion-des-tokens)
- [Limitations](#limitations)
- [Erreurs courantes](#erreurs-courantes)
- [Codes de sortie](#codes-de-sortie)
- [Contribuer](#contribuer)
- [Sécurité](#sécurité)
- [Licence](#licence)

---

## Fonctionnalités

- **Multi-comptes** — gère autant de comptes perso et de pages entreprise que voulu, chacun derrière un alias court.
- **Posts texte** — publie un post avec une audience configurable.
- **Partage de liens** — partage un article externe sous forme de carte d'aperçu ou d'image plein format, avec thumbnail optionnel (fichier local ou URL).
- **Reshares en parallèle** — republie un post depuis plusieurs comptes à la fois, en concurrence.
- **Lecture** — récupère un post, ses commentaires, ou le profil d'un membre.
- **Rafraîchissement automatique des tokens** — les access tokens sont renouvelés de façon transparente avant expiration.
- **Binaire autonome** — aucune dépendance à l'exécution, configuration dans un unique fichier JSON portable.

---

## Installation

### Binaire précompilé

Télécharger le binaire de votre plateforme depuis la page [Releases](https://github.com/z29k/rustedin/releases) et le placer dans votre `PATH`.

### Depuis les sources

Nécessite [Rust](https://rustup.rs/) (edition 2021, 1.74+).

```bash
# Build release optimisé → target/release/rustedin
cargo build --release

# Ou installer directement dans ~/.cargo/bin (disponible partout comme `rustedin`)
cargo install --path .
```

---

## Démarrage rapide

### Étape 1 — Créer les apps LinkedIn

LinkedIn impose **deux apps séparées** (restriction plateforme : la « Community Management API » doit être le *seul* produit actif sur son app).

**App pour les comptes perso**

1. [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps) → **Create App**
2. Activer les produits :
   - **Share on LinkedIn**
   - **Sign In with LinkedIn using OpenID Connect**
3. Onglet **Auth** → ajouter `http://localhost:8765/callback` comme **Redirect URL**
4. Noter le **Client ID** et le **Client Secret**

**App pour les pages entreprise**

1. [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps) → **Create App** (une deuxième app)
2. Activer le produit :
   - **Community Management API**
3. Onglet **Auth** → ajouter `http://localhost:8765/callback` comme **Redirect URL**
4. Noter le **Client ID** et le **Client Secret**

### Étape 2 — Configurer les apps

```bash
rustedin setup --app=personal     --client-id=PERSO_CLIENT_ID --client-secret=PERSO_CLIENT_SECRET
rustedin setup --app=organization --client-id=ORG_CLIENT_ID   --client-secret=ORG_CLIENT_SECRET
```

### Étape 3 — Authentifier chaque compte

```bash
# Compte perso
rustedin auth --account=quentin

# Page entreprise (l'admin de la page doit se connecter)
rustedin auth --account=mon-entreprise --org-id=TON_ORG_ID
```

> L'**org-id** se trouve dans l'URL de la page entreprise : `linkedin.com/company/MON_ORG_ID/`
>
> Pour un compte entreprise, la personne qui se connecte **doit être admin de la page**.

### Premier post

```bash
rustedin post --account=quentin --text="Mon premier post via rustedin !"
```

---

## Référence des commandes

### Option globale `--config`

Chemin vers le fichier de configuration `rustedin.json`.

```
rustedin --config <CHEMIN> <commande> [options]
```

| Option | Requis | Défaut | Description |
|--------|--------|--------|-------------|
| `--config` | non | `rustedin.json` à côté du binaire | Chemin absolu ou relatif vers le fichier de configuration |

> L'option `--config` doit apparaître **avant** le nom de la commande.

```bash
rustedin --config /etc/rustedin.json accounts
```

---

### `setup`

Configure les credentials d'une app LinkedIn. À exécuter avant `auth`.

```
rustedin setup --app=<TYPE> --client-id=<ID> --client-secret=<SECRET>
```

| Option | Requis | Description |
|--------|--------|-------------|
| `--app` | oui | Type d'app : `"personal"` ou `"organization"` |
| `--client-id` | oui | Client ID de l'app LinkedIn |
| `--client-secret` | oui | Client Secret de l'app LinkedIn |

**Comportement :** mise à jour partielle — seules les credentials du type d'app spécifié sont modifiées. Relancer pour mettre à jour.

**Sortie :** rien sur stdout. Sur stderr : `App personal credentials saved to /path/to/rustedin.json`

---

### `auth`

Authentifie un compte LinkedIn via OAuth 2.0. Ouvre le navigateur automatiquement.

```
rustedin auth --account=<NOM> [--org-id=<ID>]
```

| Option | Requis | Description |
|--------|--------|-------------|
| `--account` | oui | Alias du compte (nom libre, ex : `"quentin"`, `"mon-entreprise"`) |
| `--org-id` | non | ID de l'organisation LinkedIn (pages entreprise uniquement) |

**Détermination du type de compte :**

- `--org-id` présent → type `organization` (credentials de l'app organization)
- sinon → type `person` (credentials de l'app personal)

**Scopes OAuth demandés :**

| Type | Scopes |
|------|--------|
| `person` | `w_member_social openid profile email` |
| `organization` | `r_organization_social w_organization_social` |

> **Pourquoi pas `openid profile email` pour une org ?** LinkedIn impose que la **Community Management API** soit le *seul* produit de son app. Les scopes `openid profile email` (produit *Sign In with LinkedIn using OpenID Connect*) ne peuvent donc pas être ajoutés — les demander provoquerait une erreur `unauthorized_scope_error`.

**Déroulement :**

1. Ouvre le navigateur vers la page d'autorisation LinkedIn (ou affiche l'URL si le navigateur ne s'ouvre pas)
2. Écoute sur `http://localhost:8765/callback` (port 8765)
3. Timeout : **5 minutes** — au-delà, la commande échoue
4. Échange le code d'autorisation contre des tokens
5. **Comptes perso uniquement** : récupère le profil (`/v2/userinfo`) pour déterminer le `person_urn`. Pour une org, l'URN (`urn:li:organization:<org-id>`) est dérivé directement de `--org-id`
6. Stocke les tokens dans `rustedin.json`

**Sortie :** rien sur stdout. Sur stderr :

```
Account "quentin" saved to /path/to/rustedin.json
  Type          : person
  URN           : urn:li:person:abc123
  Authorized by : Quentin Dupont (urn:li:person:abc123)
  Access token  : 60 days (auto-refreshed)
  Refresh token : 365 days (rustedin auth once/year)
```

---

### `accounts`

Liste tous les comptes configurés avec leur état.

```
rustedin accounts
```

**Sortie (stdout) :** tableau JSON.

```json
[
  {
    "alias": "quentin",
    "type": "person",
    "urn": "urn:li:person:abc123",
    "access_token_status": "active",
    "access_token_expires_in_days": 45,
    "refresh_token_expires_in_days": 320
  }
]
```

**Si aucun compte configuré :** rien sur stdout. Sur stderr : `No accounts configured. Run: rustedin auth --account=<alias>`

---

### `status`

Affiche l'état détaillé des tokens pour chaque compte.

```
rustedin status
```

**Sortie (stdout) :** **objet** JSON (clé = alias du compte).

> Contrairement à `accounts` (tableau), `status` retourne un objet indexé par alias, et ajoute le champ `needs_reauth`.

```json
{
  "quentin": {
    "type": "person",
    "urn": "urn:li:person:abc123",
    "access_token_status": "active",
    "access_token_expires_in_days": 45,
    "refresh_token_expires_in_days": 320,
    "needs_reauth": false
  }
}
```

---

### `post`

Publie un post texte.

```
rustedin post --account=<NOM> --text=<CONTENU> [--visibility=<NIVEAU>]
```

| Option | Requis | Défaut | Description |
|--------|--------|--------|-------------|
| `--account` | oui | — | Alias du compte |
| `--text` | oui | — | Contenu du post (1 à 3000 caractères) |
| `--visibility` | non | `PUBLIC` | Audience (voir [Visibilité](#visibilité---visibility)) |

**Sortie (stdout) :** JSON avec `success`, `post_id`, `account`, `urn`, `visibility`, `text_preview` (100 premiers caractères).

```bash
rustedin post --account=quentin --text="Hello LinkedIn !"
rustedin post --account=quentin --text="Visible par mes connexions" --visibility=connections
```

---

### `reshare`

Republie un post existant depuis un ou plusieurs comptes. Les republications s'exécutent en parallèle.

```
rustedin reshare --post-id=<URN> --accounts=<LISTE> [--commentary=<TEXTE>] [--visibility=<NIVEAU>]
```

| Option | Requis | Défaut | Description |
|--------|--------|--------|-------------|
| `--post-id` | oui | — | URN du post à republier (ex : `"urn:li:share:7123456789"`) |
| `--accounts` | oui | — | Liste d'alias séparés par des virgules, ou `"*"` |
| `--commentary` | non | `""` | Commentaire au-dessus du reshare |
| `--visibility` | non | `PUBLIC` | Audience |

**Wildcard `"*"` :** se déploie vers tous les comptes `person` uniquement. Les comptes `organization` sont ignorés.

**Exécution concurrente :** toutes les republications s'exécutent en parallèle. Un échec sur un compte n'empêche pas les autres.

**Sortie (stdout) :** JSON avec `total`, `succeeded`, `failed`, et un tableau `results` par compte.

```bash
# Republier depuis tous les comptes perso
rustedin reshare --post-id="urn:li:share:7123456789" --accounts="*"

# Republier depuis des comptes spécifiques avec commentaire
rustedin reshare --post-id="urn:li:share:7123456789" --accounts=quentin,alice --commentary="À lire !"
```

---

### `share`

Partage un article ou une URL externe avec un aperçu de lien. Permet d'ajouter une image (thumbnail) via un chemin local ou une URL.

```
rustedin share --account=<NOM> --url=<URL> --title=<TITRE> \
  [--description=<DESC>] [--commentary=<TEXTE>] [--visibility=<NIVEAU>] \
  [--image=<CHEMIN_OU_URL>] [--mode=<MODE>]
```

| Option | Requis | Défaut | Description |
|--------|--------|--------|-------------|
| `--account` | oui | — | Alias du compte |
| `--url` | oui | — | URL de l'article à partager |
| `--title` | oui | — | Titre de l'article (1 à 400 caractères) |
| `--description` | non | — | Description de l'article. **Note :** LinkedIn ne l'affiche pas dans l'aperçu ; si `--commentary` n'est pas fourni, la description sert de commentaire au-dessus du lien |
| `--commentary` | non | `""` | Commentaire personnel au-dessus du lien |
| `--visibility` | non | `PUBLIC` | Audience |
| `--image` | non | — | Image de l'article : chemin local ou URL (max 10 MB). Thumbnail en mode `article`, image plein format en mode `image` |
| `--mode` | non | `article` | `article` (carte d'aperçu, défaut) ou `image` (image plein format + lien dans le texte). En mode `image`, `--image` est obligatoire |

**Détection de l'image :** si la valeur commence par `http://` ou `https://`, elle est traitée comme une URL (téléchargée puis uploadée) ; sinon, comme un chemin de fichier local.

**Sortie (stdout) :** JSON avec `success`, `post_id`, `account`, `urn`, `url`, `mode`, et `image_urn` (uniquement si `--image` a été utilisé).

```bash
# Partage simple (sans image)
rustedin share --account=quentin \
  --url="https://example.com/article" \
  --title="Un super article" \
  --commentary="Je recommande cette lecture !"

# Mode image plein format
rustedin share --account=quentin \
  --url="https://example.com/article" \
  --title="Un super article" \
  --image="https://example.com/photo.jpg" \
  --mode=image
```

---

### `get-post`

Récupère un post par son URN (utile pour inspecter ce que LinkedIn a stocké).

```
rustedin get-post --account=<NOM> --post-id=<URN>
```

| Option | Requis | Description |
|--------|--------|-------------|
| `--account` | oui | Alias du compte (pour l'authentification) |
| `--post-id` | oui | URN du post (ex : `"urn:li:share:123456"`) |

**Sortie (stdout) :** le JSON brut de l'API LinkedIn pour ce post.

---

### `comments`

Récupère les commentaires d'un post LinkedIn.

```
rustedin comments --account=<NOM> --post-id=<URN> [--count=<N>] [--start=<N>]
```

| Option | Requis | Défaut | Description |
|--------|--------|--------|-------------|
| `--account` | oui | — | Alias du compte (pour l'authentification) |
| `--post-id` | oui | — | URN du post (`urn:li:share:xxx`, `urn:li:activity:xxx` ou `urn:li:ugcPost:xxx`) |
| `--count` | non | `20` | Nombre de commentaires (1 à 100) |
| `--start` | non | `0` | Index de départ pour la pagination |

**Sortie (stdout) :** JSON avec `post_id`, `account`, `start`, `count`, `total`, et un tableau `comments`.

---

### `profile`

Récupère le profil d'un membre. Sans `--urn`, retourne ton propre profil (comptes perso uniquement).

```
rustedin profile --account=<NOM> [--urn=<PERSON_URN>]
```

| Option | Requis | Description |
|--------|--------|-------------|
| `--account` | oui | Alias du compte (pour l'authentification) |
| `--urn` | non | URN d'un autre membre (ex : `urn:li:person:xxx`) ; omettre pour ton propre profil |

> La consultation de son propre profil n'est pas disponible pour les comptes `organization` (pas de scope `openid`). Passer un `--urn` à la place.

---

## Visibilité (`--visibility`)

Disponible sur `post`, `reshare` et `share`. Contrôle qui peut voir le post.

| Valeur | Description |
|--------|-------------|
| `PUBLIC` | Visible par tout le monde **(défaut)** |
| `CONNECTIONS` | Visible uniquement par les connexions de 1er degré |
| `LOGGED_IN` | Visible uniquement par les membres connectés à LinkedIn |

> La valeur est **insensible à la casse** : `connections`, `Connections` et `CONNECTIONS` sont équivalents.

---

## Fichier de configuration (`rustedin.json`)

### Emplacement

| Méthode | Chemin |
|---------|--------|
| Par défaut | `rustedin.json` dans le même dossier que le binaire (portable) |
| Override | `rustedin --config /chemin/vers/rustedin.json <commande>` |

Le fichier est créé automatiquement s'il n'existe pas.

### Schéma

```json
{
  "linkedInApp": {
    "personal":     { "client_id": "...", "client_secret": "..." },
    "organization": { "client_id": "...", "client_secret": "..." }
  },
  "accounts": {
    "<alias>": {
      "alias": "string",
      "type": "person | organization",
      "urn": "string | null",
      "person_urn": "string | null",
      "access_token": "string",
      "refresh_token": "string",
      "expires_at": "number (Unix ms)",
      "refresh_expires_at": "number (Unix ms)"
    }
  }
}
```

> ⚠️ **Sécurité :** ce fichier contient des tokens OAuth et des client secrets. **Ne jamais le partager ni le committer.** Il est déjà listé dans `.gitignore`.

---

## Gestion des tokens

| Token | Durée de vie | Rafraîchissement |
|-------|--------------|------------------|
| Access token | 60 jours | Rafraîchi automatiquement **7 jours avant l'expiration**, de façon transparente, à chaque commande API |
| Refresh token | 365 jours | Ne peut pas être rafraîchi automatiquement — relancer `rustedin auth` une fois par an et par compte |

Quand un rafraîchissement se produit, un message apparaît sur stderr :

```
[rustedin] Refreshing token for "quentin"...
[rustedin] Token refreshed for "quentin".
```

| Situation | Symptôme | Solution |
|-----------|----------|----------|
| Access token expiré, refresh token valide | Rafraîchissement automatique | Rien à faire |
| Refresh token expiré | `Refresh token for "X" has expired...` | `rustedin auth --account=X` |
| Le refresh échoue | `Token refresh failed for "X"...` | Vérifier le réseau, puis `rustedin auth --account=X` |

---

## Limitations

1. **Port 8765 obligatoire** — le callback OAuth écoute sur `127.0.0.1:8765` (hardcodé) ; le port doit être libre pendant `auth`.
2. **Texte max 3000 caractères** — les posts texte sont limités à 3000 caractères (validation côté client).
3. **Titre article max 400 caractères** — le titre dans `share` est limité à 400 caractères.
4. **Pas d'upload vidéo** — seuls les posts texte, les partages de liens (avec image optionnelle) et les reshares sont supportés.
5. **Pas de suppression/édition de post** — une fois publié, un post ne peut pas être modifié ou supprimé via rustedin.
6. **Pas de rate limiting côté client** — si LinkedIn retourne un HTTP 429, l'erreur est remontée telle quelle ; aucun retry automatique.
7. **Pas de support proxy** — les requêtes HTTP passent directement par le réseau (pas de `HTTP_PROXY`, pas d'option CLI).
8. **Pas de variables d'environnement** — toute la configuration passe par `rustedin.json` ou le flag `--config`.
9. **Wildcard `*` = comptes person uniquement** — dans `reshare --accounts="*"`, seuls les comptes `person` sont ciblés.
10. **Redirect URI fixe** — l'URL de callback OAuth est hardcodée à `http://localhost:8765/callback` et doit être enregistrée telle quelle dans chaque app LinkedIn.
11. **Version API LinkedIn fixe** — fixée à `202603` (header `LinkedIn-Version`).
12. **Pas de timeout sur les requêtes HTTP** — les appels à l'API LinkedIn n'ont pas de timeout configuré (seul le callback OAuth a un timeout de 5 minutes).
13. **Description d'article non affichée** — LinkedIn n'affiche pas le champ `description` dans l'aperçu du lien ; si `--description` est fourni sans `--commentary`, il sert de commentaire au-dessus du lien.
14. **Caractères réservés LinkedIn** — `( ) [ ] @ # * _ ~ { } < > | \` sont réservés dans le format « little text » de LinkedIn. rustedin les échappe automatiquement avec `\` pour éviter la troncature silencieuse du texte.

---

## Erreurs courantes

| Message | Cause | Solution |
|---------|-------|----------|
| `App personal not configured. Run: rustedin setup ...` | Credentials manquantes | `rustedin setup --app=personal ...` |
| `Invalid app type "X". Use "personal" or "organization".` | Mauvaise valeur pour `--app` | Utiliser `"personal"` ou `"organization"` |
| `Account "X" not found. Available: ...` | Alias inconnu | Vérifier l'alias ou lancer `rustedin auth --account=X` |
| `No URN stored for "X". Run: rustedin auth --account=X` | Auth interrompue ou incomplète | Relancer `rustedin auth --account=X` |
| `Refresh token for "X" has expired...` | Refresh token > 365 jours | `rustedin auth --account=X` |
| `Failed to bind port 8765: ...` | Port déjà utilisé | Libérer le port 8765 (`lsof -i :8765`) |
| `Timeout waiting for OAuth callback (5 min)` | Pas de réponse du navigateur | Relancer et ouvrir l'URL manuellement |
| `Auth failed: unauthorized_scope_error` | Produit manquant sur l'app. **Perso :** activer *Share on LinkedIn* + *Sign In with LinkedIn using OpenID Connect*. **Org :** activer *Community Management API* (seul produit autorisé) | Ajouter le produit dans l'onglet *Products* de l'app, puis relancer `rustedin auth` |
| `Text must be between 1 and 3000 characters` | Texte vide ou > 3000 chars | Ajuster la longueur du texte |
| `Title must be between 1 and 400 characters` | Titre vide ou > 400 chars | Ajuster la longueur du titre |
| `LinkedIn API error STATUS for "X": ...` | LinkedIn a refusé la requête | Lire le corps de l'erreur pour le détail |
| `Image too large (X.X MB). LinkedIn allows max 10 MB.` | Image > 10 MB | Réduire la taille de l'image |

---

## Codes de sortie

| Code | Signification |
|------|---------------|
| `0` | Succès |
| `1` | Erreur (le message est affiché sur stderr) |

---

## Contribuer

Les contributions sont les bienvenues ! Lire [CONTRIBUTING.md](CONTRIBUTING.md) pour le modèle de branches (`main` / `develop`, `feature/*` et `fix/*` → PR dans `develop`), les conventions de commit, et le processus de release. En participant, vous acceptez le [Code de conduite](CODE_OF_CONDUCT.md).

---

## Sécurité

Une vulnérabilité ? Merci de suivre le processus décrit dans [SECURITY.md](SECURITY.md) — ne pas ouvrir d'issue publique pour un signalement de sécurité.

---

## Licence

Distribué sous [licence MIT](LICENSE). © 2026 z29k.
