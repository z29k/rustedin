<p align="center">
  <img src="assets/rustedin-logo.jpg" alt="logo rustedin" width="320">
</p>

<h1 align="center">rustedin</h1>

<p align="center">
  CLI social multi-comptes — publiez sur LinkedIn, les Pages Facebook et Instagram depuis un seul binaire.
</p>

<p align="center">
  <a href="https://github.com/z29k/rustedin/actions/workflows/ci.yml"><img src="https://github.com/z29k/rustedin/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/z29k/rustedin/releases"><img src="https://img.shields.io/github/v/release/z29k/rustedin?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="Licence : MIT"></a>
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
  - [LinkedIn](#linkedin)
  - [Meta — Facebook et Instagram](#meta--facebook-et-instagram)
  - [Premières publications](#premières-publications)
- [Migrer depuis la 1.x](#migrer-depuis-la-1x)
- [Référence des commandes](#référence-des-commandes)
  - [Options globales](#options-globales)
  - [`accounts`](#accounts)
  - [`status`](#status)
  - [`migrate`](#migrate)
  - [`broadcast`](#broadcast)
  - [`linkedin setup`](#linkedin-setup)
  - [`linkedin auth`](#linkedin-auth)
  - [`linkedin accounts` / `linkedin status`](#linkedin-accounts--linkedin-status)
  - [`linkedin post`](#linkedin-post)
  - [`linkedin reshare`](#linkedin-reshare)
  - [`linkedin share`](#linkedin-share)
  - [`linkedin get-post`](#linkedin-get-post)
  - [`linkedin comments`](#linkedin-comments)
  - [`linkedin profile`](#linkedin-profile)
  - [`meta setup`](#meta-setup)
  - [`meta auth`](#meta-auth)
  - [`meta token`](#meta-token)
  - [`meta accounts` / `meta status`](#meta-accounts--meta-status)
  - [`meta pages`](#meta-pages)
  - [`meta use`](#meta-use)
  - [`meta get`](#meta-get)
  - [`facebook post`](#facebook-post)
  - [`facebook photo`](#facebook-photo)
  - [`facebook video`](#facebook-video)
  - [Instagram — options communes](#instagram--options-communes)
  - [`instagram post`](#instagram-post)
  - [`instagram reel`](#instagram-reel)
  - [`instagram story`](#instagram-story)
  - [`instagram publish`](#instagram-publish)
  - [`instagram limit`](#instagram-limit)
- [Visibilité (`--visibility`)](#visibilité---visibility)
- [Fonctionnement des médias](#fonctionnement-des-médias)
- [Fichier de configuration (`rustedin.json`)](#fichier-de-configuration-rustedinjson)
- [Gestion des tokens](#gestion-des-tokens)
- [Compiler un binaire mono-plateforme](#compiler-un-binaire-mono-plateforme)
- [Limitations](#limitations)
- [Erreurs courantes](#erreurs-courantes)
- [Codes de sortie](#codes-de-sortie)
- [Contribuer](#contribuer)
- [Sécurité](#sécurité)
- [Licence](#licence)

---

## Fonctionnalités

- **Trois plateformes, un binaire** — comptes personnels et pages entreprise
  LinkedIn, Pages Facebook, comptes professionnels Instagram.
- **Multi-comptes** — autant de comptes que vous voulez, chacun derrière un
  alias court, cloisonné par plateforme : le même alias peut donc nommer une
  page LinkedIn *et* un compte Meta.
- **Publication croisée** — [`broadcast`](#broadcast) envoie le même contenu à
  toutes les plateformes en parallèle, avec un rapport par cible et un
  `--dry-run`.
- **LinkedIn** — posts texte, partages d'articles (carte de lien ou image plein
  format), republications en éventail, lecture des posts, commentaires et
  profils.
- **Pages Facebook** — posts texte et liens, photos simples ou multiples,
  vidéos, le tout programmable jusqu'à 75 jours à l'avance.
- **Instagram** — posts feed, carrousels de 2 à 10 éléments, Reels et Stories,
  y compris des fichiers locaux que l'API Instagram n'accepte pas directement.
- **Renouvellement automatique des tokens** — les tokens d'accès LinkedIn sont
  rafraîchis 7 jours avant expiration ; les tokens utilisateur Meta sont
  ré-échangés dans la même fenêtre.
- **Pensé pour être scripté** — un seul document JSON sur stdout, tout le reste
  sur stderr, et un code de sortie non nul à la moindre erreur.
- **Des retrys qui ne peuvent pas publier deux fois** — les limitations de débit
  et les erreurs serveur sont rejouées avec backoff exponentiel, mais une
  publication n'est jamais rejouée après un échec qui a pu aboutir malgré tout.

---

## Installation

### Binaire précompilé

Téléchargez l'archive correspondant à votre plateforme depuis la
[page des releases](https://github.com/z29k/rustedin/releases), extrayez-la et
placez `rustedin` dans votre `PATH`.

### Depuis les sources

```bash
git clone https://github.com/z29k/rustedin.git
cd rustedin

# Build release optimisé → target/release/rustedin
cargo build --release

# Ou installer directement dans ~/.cargo/bin (disponible partout comme `rustedin`)
cargo install --path .
```

Nécessite Rust 1.74 ou plus. Pour ne compiler qu'une seule plateforme, voir
[Compiler un binaire mono-plateforme](#compiler-un-binaire-mono-plateforme).

---

## Démarrage rapide

Ne configurez que les plateformes dont vous avez besoin : LinkedIn et Meta sont
indépendantes.

### LinkedIn

#### Étape 1 — Créer les apps LinkedIn

LinkedIn impose **deux apps distinctes** (restriction de la plateforme : la
« Community Management API » doit être le *seul* produit actif sur son app).

**App pour les comptes personnels**

1. [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps) → **Create App**
2. Activer les produits :
   - **Share on LinkedIn**
   - **Sign In with LinkedIn using OpenID Connect**
3. Onglet **Auth** → ajouter `http://localhost:8765/callback` comme **Redirect URL**
4. Copier le **Client ID** et le **Client Secret**

**App pour les pages entreprise**

1. [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps) → **Create App** (une seconde app)
2. Activer le produit :
   - **Community Management API**
3. Onglet **Auth** → ajouter `http://localhost:8765/callback` comme **Redirect URL**
4. Copier le **Client ID** et le **Client Secret**

#### Étape 2 — Configurer les apps

```bash
rustedin linkedin setup --app=personal     --client-id=CLIENT_ID_PERSO --client-secret=CLIENT_SECRET_PERSO
rustedin linkedin setup --app=organization --client-id=CLIENT_ID_ORG   --client-secret=CLIENT_SECRET_ORG
```

#### Étape 3 — Authentifier chaque compte

```bash
# Compte perso
rustedin linkedin auth --account=quentin

# Page entreprise (l'admin de la page doit se connecter)
rustedin linkedin auth --account=mon-entreprise --org-id=VOTRE_ORG_ID
```

> L'**org-id** se trouve dans l'URL de la page entreprise :
> `linkedin.com/company/MON_ORG_ID/`
>
> Pour un compte entreprise, la personne qui se connecte **doit être admin de la
> page**.

### Meta — Facebook et Instagram

Une seule authentification Facebook couvre les deux plateformes : les tokens de
Page qu'elle produit publient sur les Pages Facebook, et chaque Page porte
l'identifiant du compte professionnel Instagram qui lui est lié.

#### Étape 1 — Créer l'app Meta

1. Rendez-vous sur [developers.facebook.com/apps](https://developers.facebook.com/apps)
   et créez une app de type **Business**.
2. Ajoutez le produit **Facebook Login**. Dans *Facebook Login → Settings*,
   ajoutez ceci aux **Valid OAuth Redirect URIs**, tel quel :

   ```
   http://localhost:8765/callback
   ```

   > Meta n'accepte `http://localhost` que tant que l'app est en mode
   > **Development**. Une fois l'app en Live, l'URI de redirection doit être en
   > HTTPS : utilisez `--redirect-uri` avec un tunnel, et déclarez cette URI.

3. Ajoutez le produit **Instagram** si vous comptez publier sur Instagram.
4. Copiez l'**App ID** et l'**App Secret** dans *Settings → Basic*.

Prérequis côté comptes :

- l'utilisateur qui s'authentifie doit avoir un rôle sur chaque Page Facebook ;
- chaque compte Instagram doit être un compte **professionnel** (Business ou
  Creator) et être **lié à une Page Facebook** dans Meta Business Suite.

En mode Development, tout fonctionne pour les utilisateurs ayant un rôle sur
l'app. Passer en Live nécessite une App Review pour `pages_manage_posts` et
`instagram_content_publish`.

#### Quel chemin d'authentification est le vôtre

Les apps Business embarquent **Facebook Login for Business**, conçu pour un
prestataire technique qui obtient l'accès aux ressources d'un *client*. Cette
distinction décide de la façon dont vous vous authentifiez, et se tromper de
chemin coûte un après-midi :

| À qui appartiennent les Pages ? | Commande | Pourquoi |
| ------------------------------- | -------- | -------- |
| **À vous** (vous possédez l'app *et* les Pages) | [`meta token`](#meta-token) | Login for Business refuse de déléguer des ressources au portefeuille propriétaire de l'app : le sélecteur grise ce portefeuille, `auth` n'a donc aucun chemin |
| **À vos clients** (vous êtes prestataire) | [`meta auth`](#meta-auth) avec `--config-id` | Le flux de délégation standard |

Publier sur ses propres Pages est le cas courant, et il contourne complètement
OAuth : générez un token d'utilisateur système comme décrit dans
[`meta token`](#meta-token) et passez directement aux
[premières publications](#premières-publications). La suite de cette section
couvre le flux client.

> Pour le flux client, créez la configuration dans *Facebook Login for
> Business → Configurations* — [`config_id` a remplacé `scope`](https://developers.facebook.com/docs/facebook-login/facebook-login-for-business),
> et sans elle Meta accorde les permissions mais n'attache aucune Page, laissant
> `/me/accounts` vide. Réglez le type de token sur **System user**, pas User :
> Meta n'affiche le sélecteur de portefeuille et de ressources que pour les
> configurations « system user », celles qui accordent un *« accès continu aux
> ressources professionnelles, telles que les Pages Facebook, les comptes
> publicitaires ou les comptes Instagram »*.

#### Étape 2 — Configurer l'app

```bash
rustedin meta setup --app-id=1234567890 --app-secret=abcdef... --config-id=9876543210
```

`--config-id` est optionnel : omettez-le sur les apps utilisant le Facebook
Login classique, où `auth` demande les scopes par défaut à la place.

#### Étape 3 — S'authentifier

```bash
rustedin meta auth --account=z29k
```

Votre navigateur ouvre l'écran de consentement Meta ; rustedin attend le retour
sur `http://localhost:8765`, échange le code contre un token longue durée et
enregistre chaque Page que vous administrez avec son compte Instagram lié.

```bash
rustedin meta pages --account=z29k
```

### Premières publications

```bash
# LinkedIn
rustedin linkedin post --account=quentin --text="Mon premier post via rustedin !"

# Facebook
rustedin facebook post --account=z29k --message="Bonjour depuis rustedin"

# Instagram
rustedin instagram post --account=z29k --media=./photo.jpg --caption="Bonjour 👋"

# Les trois d'un coup
rustedin broadcast \
  --to=linkedin:quentin,facebook:z29k,instagram:z29k \
  --text="Bonjour depuis toutes les plateformes" \
  --image=./photo.jpg
```

---

## Migrer depuis la 1.x

rustedin 2.0 regroupe chaque commande sous sa plateforme et cloisonne le fichier
de configuration, pour qu'un alias LinkedIn et un alias Meta puissent porter le
même nom.

**Commandes** — chaque commande 1.x passe sous `linkedin` (le groupe répond
aussi à `li`) :

| 1.x | 2.0 |
|-----|-----|
| `rustedin setup --app=…` | `rustedin linkedin setup --app=…` |
| `rustedin auth --account=…` | `rustedin linkedin auth --account=…` |
| `rustedin accounts` | `rustedin linkedin accounts`, ou `rustedin accounts` pour toutes les plateformes |
| `rustedin status` | `rustedin linkedin status`, ou `rustedin status` pour toutes les plateformes |
| `rustedin post` | `rustedin linkedin post` |
| `rustedin reshare` | `rustedin linkedin reshare` |
| `rustedin share` | `rustedin linkedin share` |
| `rustedin get-post` | `rustedin linkedin get-post` |
| `rustedin comments` | `rustedin linkedin comments` |
| `rustedin profile` | `rustedin linkedin profile` |

**Configuration** — un `rustedin.json` 1.x est migré automatiquement à la
première lecture par la 2.0. Rien à faire à la main.

**Vous venez de `rustameta`** — fusionnez sa configuration dans celle-ci, tokens
compris :

```bash
rustedin migrate --from /chemin/vers/rustameta.json
```

---

## Référence des commandes

Chaque commande affiche **un seul document JSON sur stdout**. La progression,
les avertissements et les traces de requêtes vont sur stderr : `rustedin … | jq`
ne voit donc jamais une ligne de log.

### Options globales

| Option | Description |
|--------|-------------|
| `--config <CHEMIN>` | Fichier de configuration à utiliser. Par défaut `rustedin.json` à côté du binaire |
| `--api-version <V>` | Version de la Graph API, ex. `v25.0`. Meta uniquement |
| `--help`, `--version` | Les classiques |

Les options globales sont acceptées **à n'importe quelle position** : avant le
groupe de plateforme, après lui, ou après la sous-commande.

```bash
rustedin --config=/etc/rustedin.json facebook post --account=z29k --message="…"
rustedin facebook post --account=z29k --message="…" --config=/etc/rustedin.json
```

Les groupes de plateformes acceptent des alias courts : `li`, `fb`, `ig`.

Toutes les commandes Facebook et Instagram acceptent `--account` et `--page`.
`--page` est optionnel quand le compte n'a qu'une Page ou qu'une Page par défaut
est épinglée avec [`meta use`](#meta-use) ; il accepte un ID de Page, un nom
exact, ou un fragment de nom unique (insensible à la casse).

---

### `accounts`

Tous les comptes configurés, groupés par plateforme.

```bash
rustedin accounts
```

```json
{
  "linkedin": [
    {
      "platform": "linkedin",
      "alias": "quentin",
      "type": "person",
      "urn": "urn:li:person:xxxx",
      "access_token_status": "active",
      "access_token_expires_in_days": 52,
      "access_token_expires_on": "2026-10-23",
      "refresh_token_expires_in_days": 341,
      "refresh_token_expires_on": "2027-08-08",
      "needs_reauth": false
    }
  ],
  "meta": [
    {
      "platform": "meta",
      "alias": "z29k",
      "user_id": "1220000000000000",
      "name": "Quentin Mathis",
      "user_token_status": "active",
      "user_token_expires_in_days": 3649,
      "user_token_expires_on": "2036-08-28",
      "needs_reauth": false,
      "pages": 1,
      "instagram_accounts": 1,
      "scopes": ["pages_show_list", "pages_manage_posts", "..."]
    }
  ]
}
```

### `status`

Les mêmes informations, indexées par alias, par plateforme.

```bash
rustedin status
rustedin status --check   # valide en plus les tokens Meta via /debug_token
```

### `migrate`

Fusionne un autre fichier de configuration dans celui-ci. Conçue pour le passage
1.x → 2.0 : pointez-la vers un `rustameta.json` et ses identifiants d'app et ses
comptes atterrissent sous la clé `meta`. Un `rustedin.json` 1.x fonctionne aussi
bien.

| Option | Requis | Description |
|--------|--------|-------------|
| `--from <CHEMIN>` | oui | Fichier à importer. Il n'est jamais modifié |
| `--force` | non | Écrase les comptes et identifiants déjà présents ici |

```bash
rustedin migrate --from ~/rustameta.json
```

```json
{
  "success": true,
  "from": "/Users/moi/rustameta.json",
  "into": "/usr/local/bin/rustedin.json",
  "imported_accounts": ["meta:z29k"],
  "imported_app_credentials": ["meta"],
  "skipped_accounts": []
}
```

Sans `--force`, un compte déjà présent est listé dans `skipped_accounts` et
laissé intact.

### `broadcast`

Un contenu, plusieurs plateformes, publiées en parallèle.

| Option | Requis | Description |
|--------|--------|-------------|
| `--to <CIBLES>` | oui | Cibles `plateforme:compte[/page]` séparées par des virgules |
| `--text <TEXTE>` | non | Commentaire LinkedIn, message Facebook, légende Instagram |
| `--image <CHEMIN\|URL>` | non | Fichier local ou URL publique. **Obligatoire** pour une cible Instagram |
| `--link <URL>` | non | URL à attacher |
| `--title <TITRE>` | non | Titre là où une plateforme en exige un. Par défaut, la première ligne de `--text` |
| `--visibility <V>` | non | Audience LinkedIn. Par défaut `PUBLIC` |
| `--schedule <QUAND>` | non | Facebook uniquement. Timestamp Unix ou date ISO 8601 |
| `--alt-text <TEXTE>` | non | Instagram uniquement |
| `--cleanup-relay` | non | Supprime les photos Facebook temporaires servant à relayer les images locales |
| `--dry-run` | non | Affiche ce que chaque cible publierait, sans rien publier |

Une cible nomme une plateforme, un alias de compte et — pour Facebook et
Instagram — une Page optionnelle :

```
linkedin:quentin          li:quentin
facebook:z29k             fb:z29k/Ma Page
instagram:z29k            ig:z29k
```

Le même contenu est adapté à ce que chaque plateforme accepte réellement :

| | texte seul | + image | + lien |
|---|---|---|---|
| **LinkedIn** | post texte | post image | carte article (l'image devient la vignette) |
| **Facebook** | post feed | post photo | aperçu de lien (sur un post photo, l'URL passe dans la légende) |
| **Instagram** | *refusé* | post feed | ajouté à la légende en texte brut |

> **Pourquoi Instagram n'a droit qu'au texte brut.** L'API de publication
> n'expose aucun champ de lien : un conteneur accepte `image_url` / `video_url`,
> `media_type`, `caption` et quelques drapeaux, rien qui attache une URL.
> Instagram ne transforme pas non plus une URL de légende en hyperlien — un
> choix délibéré, pour garder les gens dans l'app. Depuis mars 2026 des légendes
> cliquables sont en test pour les créateurs professionnels Meta Verified, mais
> uniquement dans l'app mobile et pas via l'API. rustedin ajoute donc l'URL et
> avertit ; il n'y a pas d'autre option.

```bash
rustedin broadcast \
  --to=linkedin:quentin,linkedin:z29k,facebook:z29k,instagram:z29k \
  --text="Nouvel article sur le blog" \
  --link=https://z29k.fr/blog/rustedin \
  --image=./couverture.jpg
```

```json
{
  "total": 4,
  "succeeded": 3,
  "failed": 1,
  "results": [
    { "target": "linkedin:quentin", "success": true, "platform": "linkedin",
      "kind": "article", "post_id": "urn:li:share:7123", "image_urn": "urn:li:image:C1" },
    { "target": "facebook:z29k", "success": true, "platform": "facebook",
      "kind": "photo", "post_id": "1111_2222",
      "permalink": "https://www.facebook.com/1111_2222" },
    { "target": "instagram:z29k", "success": false,
      "error": "POST /media: Graph API 400 — ... [code 9004]" }
  ]
}
```

Les échecs sont par cible : un refus n'annule jamais les autres. Le processus
sort en `1` si **au moins une** cible a échoué — un échec partiel n'est donc
jamais silencieux.

Les options qu'une cible ne peut pas honorer déclenchent un avertissement sur
stderr plutôt qu'un refus : `--schedule` sur une cible LinkedIn, un `--link` sur
Instagram. Le seul refus ferme est une cible Instagram sans `--image`, vérifié
avant toute publication.

---

### `linkedin setup`

Enregistre les identifiants de l'une des deux apps LinkedIn.

| Option | Requis | Description |
|--------|--------|-------------|
| `--app <TYPE>` | oui | `personal` ou `organization` |
| `--client-id <ID>` | oui | Client ID de l'app |
| `--client-secret <SECRET>` | oui | Client Secret de l'app |

```bash
rustedin linkedin setup --app=personal --client-id=86xxxx --client-secret=WPL_AP1.xxxx
```

Mise à jour partielle : seuls les identifiants du type d'app indiqué changent.
Les deux apps se configurent donc indépendamment, et l'une comme l'autre peut
être relancée pour faire tourner un secret.

### `linkedin auth`

OAuth 2.0 avec ouverture automatique du navigateur et écoute locale du callback.

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Alias sous lequel enregistrer le compte |
| `--org-id <ID>` | non | ID d'organisation — en fait une page entreprise |
| `--port <PORT>` | non | Port du callback. Par défaut `8765` |

```bash
rustedin linkedin auth --account=quentin
rustedin linkedin auth --account=z29k --org-id=107692414
```

Scopes demandés :

| Type de compte | Scopes |
|----------------|--------|
| Personnel | `w_member_social openid profile email` |
| Organisation | `r_organization_social w_organization_social` |

Le type de compte se déduit de `--org-id` : présent → `organization`, avec les
identifiants de l'app organisation ; absent → `person`, avec ceux de l'app
personnelle.

> **Pourquoi pas `openid profile email` pour une organisation ?** LinkedIn exige
> que la **Community Management API** soit le *seul* produit de son app. Or les
> scopes `openid profile email` proviennent de *Sign In with LinkedIn using
> OpenID Connect*, qui ne peut donc pas être ajouté — les demander déclenche
> `unauthorized_scope_error`. C'est aussi pour cela qu'un compte organisation n'a
> pas de « profil propre » à consulter.

Le déroulé : le navigateur ouvre la page d'autorisation LinkedIn (ou l'URL est
affichée), rustedin écoute sur `http://localhost:<port>/callback` pendant
**5 minutes**, échange le code contre les tokens, résout l'URN — via
`/v2/userinfo` pour un compte personnel, directement depuis `--org-id` pour une
organisation — et enregistre le tout dans `rustedin.json`.

### `linkedin accounts` / `linkedin status`

La tranche LinkedIn des commandes racine [`accounts`](#accounts) et
[`status`](#status), affichée seule.

### `linkedin post`

Publier un post texte.

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Compte au nom duquel publier |
| `--text <TEXTE>` | oui | Jusqu'à 3000 caractères |
| `--visibility <V>` | non | Voir [Visibilité](#visibilité---visibility). Par défaut `PUBLIC` |

```bash
rustedin linkedin post --account=quentin --text="Bonjour LinkedIn 👋"
```

```json
{
  "success": true,
  "platform": "linkedin",
  "post_id": "urn:li:share:7123456789",
  "account": "quentin",
  "urn": "urn:li:person:xxxx",
  "visibility": "PUBLIC",
  "text_preview": "Bonjour LinkedIn 👋"
}
```

### `linkedin reshare`

Republier un post existant depuis un ou plusieurs comptes, en parallèle.

| Option | Requis | Description |
|--------|--------|-------------|
| `--post-id <URN>` | oui | Post à republier, ex. `urn:li:share:7123456789` |
| `--accounts <A,B>` | oui | Alias séparés par des virgules, ou `*` pour tous les comptes personnels |
| `--commentary <TEXTE>` | non | Commentaire au-dessus de la republication |
| `--visibility <V>` | non | Par défaut `PUBLIC` |

```bash
# Republier depuis tous les comptes perso
rustedin linkedin reshare --post-id=urn:li:share:7123456789 --accounts="*"

# Depuis des comptes spécifiques, avec un commentaire
rustedin linkedin reshare --post-id=urn:li:share:7123456789 \
  --accounts=quentin,z29k --commentary="À lire"
```

### `linkedin share`

Partager un article ou une URL externe.

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Compte au nom duquel publier |
| `--url <URL>` | oui | URL de l'article |
| `--title <TITRE>` | oui | Jusqu'à 400 caractères |
| `--description <TEXTE>` | non | Utilisée comme commentaire si `--commentary` est absent |
| `--commentary <TEXTE>` | non | Commentaire personnel au-dessus de l'article |
| `--visibility <V>` | non | Par défaut `PUBLIC` |
| `--image <CHEMIN\|URL>` | non | Fichier local ou URL, 10 Mo max |
| `--mode <MODE>` | non | `article` (carte de lien, par défaut) ou `image` (image plein format) |

```bash
# Partage simple (sans image)
rustedin linkedin share --account=quentin \
  --url=https://z29k.fr/blog/rustedin --title="rustedin 2.0"

# Mode image plein format
rustedin linkedin share --account=quentin \
  --url=https://z29k.fr/blog/rustedin --title="rustedin 2.0" \
  --image=./couverture.jpg --mode=image
```

En mode `article`, l'image devient la vignette de la carte de lien. En mode
`image`, l'image occupe tout le post et l'URL est ajoutée au texte — LinkedIn
n'y affiche pas de carte.

### `linkedin get-post`

Récupérer un post par son URN, pour vérifier ce que LinkedIn a réellement
enregistré.

```bash
rustedin linkedin get-post --account=quentin --post-id=urn:li:share:7123456789
```

### `linkedin comments`

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Compte utilisé pour l'authentification |
| `--post-id <URN>` | oui | `urn:li:share:…`, `urn:li:activity:…` ou `urn:li:ugcPost:…` |
| `--count <N>` | non | 1 à 100. Par défaut `20` |
| `--start <N>` | non | Décalage de pagination. Par défaut `0` |

### `linkedin profile`

Votre propre profil, ou celui d'un autre membre.

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Compte utilisé pour l'authentification |
| `--urn <URN>` | non | URN d'un autre membre. Omettre pour votre profil |

Les comptes organisation n'ont pas le scope `openid` — la Community Management
API doit être le seul produit sur cette app — ils ne peuvent donc que consulter
un membre par URN, jamais leur « propre » profil.

---

### `meta setup`

| Option | Requis | Description |
|--------|--------|-------------|
| `--app-id <ID>` | oui | App ID Meta |
| `--app-secret <SECRET>` | oui | App Secret Meta |
| `--config-id <ID>` | non | ID de configuration Facebook Login for Business |

### `meta auth`

| Option | Requis | Défaut | Description |
|--------|--------|--------|-------------|
| `--account <ALIAS>` | oui | — | Alias sous lequel enregistrer le compte |
| `--port <PORT>` | non | `8765` | Port du serveur de callback local |
| `--redirect-uri <URI>` | non | `http://localhost:<port>/callback` | Remplace l'URI de redirection (tunnel HTTPS pour une app Live) |
| `--scopes <A,B>` | non | voir ci-dessous | Scopes séparés par des virgules, à la place des scopes par défaut |
| `--config-id <ID>` | non | celui enregistré | ID de configuration Login for Business, envoyé à la place de `scope` |
| `--no-browser` | non | `false` | Affiche l'URL d'autorisation au lieu d'ouvrir un navigateur |

Scopes par défaut :

```
pages_show_list, pages_read_engagement, pages_manage_posts,
instagram_basic, instagram_content_publish
```

Le déroulé : code d'autorisation → token courte durée → token longue durée
(~60 jours) → `/me` → `/me/permissions` → `/me/accounts` (Pages, tokens de Page,
comptes Instagram liés). Le serveur de callback expire au bout de 5 minutes.

### `meta token`

Adopter un token d'accès généré hors du flux de connexion. C'est la voie à
suivre quand vous possédez à la fois l'app et les ressources : Facebook Login
for Business refuse de déléguer des ressources au portefeuille propriétaire de
l'app, donc [`meta auth`](#meta-auth) ne peut tout simplement pas servir ce cas.
Générez plutôt un token d'**utilisateur système**.

Dans *Paramètres d'entreprise → Utilisateurs → Utilisateurs système*, ajoutez-en
un, puis **affectez des ressources trois fois** — l'app est aussi indispensable
que les Pages, et c'est son oubli qui produit une liste de permissions vide au
moment de générer le token :

1. **Apps** → votre app, en *Développeur* ou *Admin* — c'est ce qui rend une
   permission cochable
2. **Pages** → votre Page, en contrôle total
3. **Comptes Instagram** → votre compte

Puis *Générer un token*, en choisissant l'app, une expiration **Jamais**, et les
scopes listés sous [`meta auth`](#meta-auth). Les tokens d'utilisateur système
n'expirent pas : c'est une opération unique.

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Alias sous lequel enregistrer le compte |
| `--token <TOKEN>` | non | Le token. Omettez-le pour le lire sur stdin |

```bash
rustedin meta token --account=z29k              # collez le token, puis Ctrl-D
pbpaste | rustedin meta token --account=z29k    # ou passez-le par un pipe
```

Omettre `--token` évite de laisser le secret dans l'historique du shell.

### `meta accounts` / `meta status`

La tranche Meta des commandes racine. `meta status --check` demande en plus à
`/debug_token` si chaque token est encore valide, et renvoie la réponse de Meta
telle quelle sous `debug_token`.

### `meta pages`

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Alias du compte |
| `--refresh` | non | Récupère les Pages depuis Meta au lieu de lire celles enregistrées |

Lancez-la avec `--refresh` après avoir lié un nouveau compte Instagram à une
Page, ou après avoir obtenu un rôle sur une nouvelle Page.

### `meta use`

Épingler la Page par défaut d'un compte, pour que les commandes de publication
puissent se passer de `--page`.

```bash
rustedin meta use --account=z29k --page="Ma Page"
rustedin meta use --account=z29k              # efface la valeur par défaut
```

`--page` accepte un ID de Page, un nom exact, ou un fragment de nom unique
(insensible à la casse).

### `meta get`

Lecture brute de la Graph API — la porte de sortie pour tout ce que rustedin
n'encapsule pas.

```bash
rustedin meta get --account=z29k --path=/me/accounts --query=fields=id,name
```

Utilise le token de Page quand une Page est en jeu, le token utilisateur sinon.

---

### `facebook post`

Post texte et/ou lien sur une Page. Au moins l'un de `--message` / `--link` est
requis.

| Option | Requis | Description |
|--------|--------|-------------|
| `--account <ALIAS>` | oui | Alias du compte |
| `--page <ID\|NOM>` | non | Page cible. Optionnel avec une seule Page ou une Page par défaut |
| `--message <TEXTE>` | non | Texte du post |
| `--link <URL>` | non | URL à attacher en aperçu de lien |
| `--schedule <QUAND>` | non | Timestamp Unix ou date ISO 8601, de 10 min à 75 jours à l'avance |
| `--draft` | non | Crée le post non publié |

```bash
rustedin facebook post --account=z29k --message="Bonjour" --link=https://z29k.fr
```

### `facebook photo`

Une photo, ou plusieurs dans un même post. Répétez `--image` pour un post
multi-photos : chaque photo est envoyée non publiée puis assemblée via
`attached_media`.

| Option | Requis | Description |
|--------|--------|-------------|
| `--image <CHEMIN\|URL>` | oui | Répétable |
| `--message <TEXTE>` | non | Texte du post (la légende pour une photo seule) |
| plus | | `--account`, `--page`, `--schedule`, `--draft` |

### `facebook video`

| Option | Requis | Description |
|--------|--------|-------------|
| `--video <CHEMIN\|URL>` | oui | Fichier local ou URL publique |
| `--title <TITRE>` | non | Titre de la vidéo |
| `--description <TEXTE>` | non | Description de la vidéo |
| plus | | `--account`, `--page`, `--schedule`, `--draft` |

---

### Instagram — options communes

Chaque commande de publication Instagram accepte celles-ci, en plus de
`--account` et `--page` :

| Option | Description |
|--------|-------------|
| `--caption <TEXTE>` | Jusqu'à 2200 caractères |
| `--media-type <TYPE>` | `image` ou `video`, quand l'extension est ambiguë |
| `--alt-text <TEXTE>` | Texte d'accessibilité (images uniquement) |
| `--location-id <ID>` | ID de la Page Facebook de lieu à taguer |
| `--collaborators <A,B>` | Noms d'utilisateur Instagram à inviter comme collaborateurs |
| `--cover-url <URL>` | URL publique d'une couverture vidéo personnalisée |
| `--thumb-offset <MS>` | Millisecondes dans la vidéo pour la vignette |
| `--ai-generated` | Marque le post comme généré par IA |
| `--cleanup-relay` | Supprime les photos relais une fois le post en ligne |

### `instagram post`

Un post feed.

| Option | Requis | Description |
|--------|--------|-------------|
| `--media <CHEMIN\|URL>` | oui | Fichier local ou URL publique ; **répétable** (2 à 10 → carrousel) |

Deux `--media` ou plus font un carrousel ; l'alias `instagram carousel` se lit
mieux dans ce cas.

```bash
rustedin instagram post --account=z29k --media=./photo.jpg --caption="Bonjour 👋"

rustedin instagram post --account=z29k \
  --media=./1.jpg --media=./2.jpg --media=./3.jpg \
  --caption="Carrousel 📸" --cleanup-relay
```

Une vidéo passée à `instagram post` est publiée en Reel — c'est le comportement
d'Instagram, pas un choix de rustedin.

### `instagram reel`

Identique à ci-dessus avec un seul `--media`, qui doit être une vidéo.

### `instagram story`

Une Story de 24 h, un seul `--media`. `--share-to-feed` la pousse aussi sur le
fil principal.

### `instagram publish`

Publier un conteneur déjà créé — le chemin de récupération quand le traitement a
dépassé le délai d'attente.

```bash
rustedin instagram publish --account=z29k --creation-id=17900000000000000
```

### `instagram limit`

Le quota de publication sur la fenêtre glissante de 24 h. Meta autorise 100
publications via l'API ; **un carrousel ne compte que pour une**.

---

## Visibilité (`--visibility`)

LinkedIn uniquement. Disponible sur `linkedin post`, `linkedin reshare`,
`linkedin share` et `broadcast`.

| Valeur | Description |
|--------|-------------|
| `PUBLIC` | Visible par tout le monde **(par défaut)** |
| `CONNECTIONS` | Visible uniquement par les relations de 1er niveau |
| `LOGGED_IN` | Visible uniquement par les membres LinkedIn connectés |

> La valeur est **insensible à la casse** : `connections`, `Connections` et
> `CONNECTIONS` sont équivalents.

---

## Fonctionnement des médias

Les trois plateformes acceptent les médias de façons très différentes, et c'est
de loin la principale source de confusion.

| Entrée | LinkedIn | Facebook | Instagram |
|--------|----------|----------|-----------|
| URL publique (image) | **Téléchargée** par rustedin, puis envoyée | Envoyée en `url`, récupérée par Meta | Envoyée en `image_url`, récupérée par Meta |
| URL publique (vidéo) | *non supporté* | Envoyée en `file_url`, récupérée par Meta | Envoyée en `video_url`, récupérée par Meta |
| Fichier local (image) | Envoyé sur `/rest/images` | Envoyé en multipart `source` | **Relayé** par la Page — voir ci-dessous |
| Fichier local (vidéo) | *non supporté* | Envoyé en multipart `source` | Upload résumable vers `rupload.facebook.com` |

**LinkedIn ne prend que des octets.** Son endpoint d'upload n'a aucun mode
« va chercher cette URL » : une `--image` distante est donc téléchargée d'abord,
puis transmise.

**Le relais Instagram.** Instagram est l'exact inverse : il n'a aucun endpoint
acceptant des octets d'image — un conteneur ne peut que pointer vers une URL.
Quand vous passez une image locale, rustedin l'envoie donc à la Page Facebook
liée en tant que photo **non publiée**, relit son URL CDN, et alimente le
conteneur avec. Ces photos relais restent dans la photothèque de la Page ; leurs
IDs figurent dans la sortie sous `relay_photo_ids`, et `--cleanup-relay` les
supprime une fois le post en ligne.

Tout ce qu'Instagram ingère doit être **public, en HTTPS, sans redirection ni
authentification** — le récupérateur de Meta ne porte pas vos identifiants.
Instagram ne supporte officiellement que le **JPEG** pour les images ; rustedin
avertit sur tout autre format plutôt que de refuser, car Meta transcode parfois.

Plafonds vérifiés localement (chaque plateforme applique les vrais côté
serveur) :

| Endpoint | Plafond |
|----------|---------|
| Image LinkedIn | 10 Mo |
| Photo Facebook | 25 Mo |
| Vidéo Facebook | 1 Go |
| Image Instagram | 8 Mo |
| Vidéo Instagram | 1 Go |

Le type de média est déduit de l'extension du fichier. Pour les URLs sans
extension — courant avec les CDN et les liens signés — passez
`--media-type=image|video`.

---

## Fichier de configuration (`rustedin.json`)

### Emplacement

| Méthode | Chemin |
|---------|--------|
| Par défaut | `rustedin.json` à côté du binaire (portable) |
| Personnalisé | `rustedin --config /chemin/vers/rustedin.json <commande>` |

Créé automatiquement, écrit en `0600` sur Unix. Un fichier **malformé** est une
erreur et non une réinitialisation silencieuse : l'écraser détruirait la seule
copie de vos tokens.

### Schéma

Chaque plateforme possède sa clé, de sorte que le même alias peut nommer une
page entreprise LinkedIn et un compte Meta sans collision. Les clés que rustedin
ne connaît pas sont conservées telles quelles à l'écriture.

```json
{
  "linkedin": {
    "app": {
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
  },
  "meta": {
    "app": {
      "app_id": "1234567890",
      "app_secret": "...",
      "config_id": "9876543210"
    },
    "accounts": {
      "<alias>": {
        "alias": "string",
        "user_id": "10000000000000",
        "name": "Quentin Mathis",
        "access_token": "string",
        "expires_at": "number (Unix ms)",
        "scopes": ["pages_show_list", "..."],
        "default_page": "111111111111111",
        "pages": [
          {
            "id": "111111111111111",
            "name": "z29k",
            "category": "Software company",
            "access_token": "string",
            "tasks": ["CREATE_CONTENT", "MANAGE"],
            "instagram": { "id": "17841400000000000", "username": "@z29k" }
          }
        ]
      }
    }
  }
}
```

Un **fichier 1.x est migré automatiquement à la lecture** — `linkedInApp` et la
map `accounts` de premier niveau passent sous `linkedin`. Idem pour un
`rustameta.json` pointé par `--config`, dont `metaApp` passe sous `meta`.

> ⚠️ **Sécurité :** ce fichier contient des tokens OAuth, des tokens de Page et
> des client secrets. **Ne le partagez jamais et ne le committez jamais.** Il est
> déjà listé dans `.gitignore`.

---

## Gestion des tokens

### LinkedIn

| Token | Durée de vie | Renouvellement |
|-------|--------------|----------------|
| Token d'accès | 60 jours | Rafraîchi automatiquement **7 jours avant expiration**, à la première commande API |
| Refresh token | 365 jours | Non renouvelable — lancez `rustedin linkedin auth` une fois par an et par compte |

Lors d'un rafraîchissement, un message apparaît sur stderr :

```
[rustedin] Refreshing token for "quentin"...
[rustedin] Token refreshed for "quentin".
```

### Meta

| Token | Durée de vie | Renouvellement |
|-------|--------------|----------------|
| Token utilisateur | ~60 jours | Ré-échangé automatiquement dans les 7 jours précédant l'expiration |
| Token d'utilisateur système | Illimitée | Rien à faire ; relancez `meta token` si vous le révoquez |
| Token de Page | Illimitée | Réémis par `meta auth`, ou par `meta pages --refresh` |

Facebook n'a pas de mécanisme de refresh token. Un token utilisateur longue
durée se renouvelle en l'échangeant contre un nouveau *tant qu'il est encore
valide*, ce que rustedin fait de façon transparente à la première commande
lancée dans la fenêtre de 7 jours. Si cet échange échoue, c'est un
**avertissement**, pas une erreur : le token courant fonctionne jusqu'à
`expires_at`.

Comme les tokens de Page n'expirent pas, la publication continue de fonctionner
même après l'expiration du token utilisateur. Ce qui cesse de fonctionner, c'est
la découverte : `meta pages --refresh` et `meta auth` exigent tous deux un token
utilisateur valide.

Les tokens sont également invalidés quand l'utilisateur change son mot de passe
Facebook, retire l'app, ou qu'un admin révoque une permission. Dans tous les
cas, la réponse est `rustedin meta auth --account=<alias>`.

Faites le point à tout moment :

```bash
rustedin status --check
```

---

## Compiler un binaire mono-plateforme

Chaque plateforme est une feature Cargo, les deux activées par défaut :

```bash
# LinkedIn uniquement
cargo build --release --no-default-features --features linkedin

# Facebook + Instagram uniquement
cargo build --release --no-default-features --features meta
```

Un build avec une plateforme désactivée **conserve malgré tout l'intégralité du
fichier de configuration** : il ne peut donc jamais effacer les identifiants de
la plateforme qu'il ne voit pas. Un build sans aucune plateforme est refusé à la
compilation.

---

## Limitations

**Partout**

1. **Pas de support des proxys** ni de timeout HTTP configurable (connexion
   15 s, requête 600 s).
2. **Pas de variables d'environnement** — toute la configuration passe par
   `rustedin.json` ou `--config`.
3. **Ni édition ni suppression** des posts publiés.
4. Le callback OAuth écoute sur `127.0.0.1:<port>` ; le port doit être libre
   pendant `auth`, et l'URI de redirection doit être déclarée telle quelle dans
   l'app.

**LinkedIn**

5. **Pas d'upload vidéo** — uniquement posts texte, partages d'articles (avec
   image optionnelle) et republications.
6. **Texte 3000 caractères max**, titre d'article 400 max, image 10 Mo max, le
   tout validé côté client et compté en caractères.
7. **Description d'article non affichée** — LinkedIn n'affiche pas le champ
   `description` dans les aperçus de lien du fil ; si `--description` est fourni
   sans `--commentary`, il sert de commentaire au-dessus du lien.
8. **Caractères réservés** — `( ) [ ] @ # * _ ~ { } < > | \` sont réservés dans
   le format « little text » de LinkedIn. rustedin les échappe automatiquement
   pour éviter une troncature silencieuse du texte.
9. **Le joker `*` = comptes personnels uniquement** dans
   `linkedin reshare --accounts=*`.
10. **Version d'API figée** — épinglée à `202603` (en-tête `LinkedIn-Version`).

**Meta**

11. **Instagram ne peut pas recevoir d'images locales directement** — elles sont
    relayées par la Page Facebook liée (voir
    [Fonctionnement des médias](#fonctionnement-des-médias)).
12. **Instagram n'a pas d'API de programmation.** `--schedule` est réservé à
    Facebook.
13. **Pas de tag d'utilisateurs sur Instagram** (`user_tags`) ni de tags
    produits/shopping.
14. **Aucun lien cliquable sur Instagram, nulle part.** L'API de publication n'a
    pas de champ de lien : un `--link` finit en texte brut dans la légende, et
    le sticker de lien des Stories — qui fonctionne pourtant dans l'app — n'est
    pas davantage exposé par l'API.
15. **Pas d'upload vidéo résumable sur Facebook.** Au-delà de ~1 Go, passez la
    vidéo en URL via `--video` pour que Meta la récupère.
16. **Pas de chemin Instagram Login.** rustedin s'authentifie via Facebook
    Login, ce qui impose que chaque compte Instagram soit lié à une Page
    Facebook.
17. **Les photos relais sont conservées par défaut.** Utilisez
    `--cleanup-relay` pour les supprimer.
18. **La publication Instagram est séquentielle**, y compris pour les éléments
    d'un carrousel.

---

## Erreurs courantes

### Configuration et comptes

| Message | Cause | Solution |
|---------|-------|----------|
| `LinkedIn app "personal" not configured` | Identifiants manquants | `rustedin linkedin setup --app=personal …` |
| `Meta app not configured` | `meta setup` jamais lancé | `rustedin meta setup --app-id=… --app-secret=…` |
| `LinkedIn account "X" not found. Available: …` | Alias inconnu | Vérifiez l'alias, ou lancez `rustedin linkedin auth --account=X` |
| `Meta account "X" not found. Available: …` | Alias inconnu | Idem, avec `rustedin meta auth` |
| `No URN stored for "X"` | Authentification interrompue ou incomplète | Relancez `rustedin linkedin auth --account=X` |
| `Account "X" has N Pages — pass --page=<id\|name>` | Cible ambiguë | Ajoutez `--page`, ou épinglez-en une avec `rustedin meta use` |
| `Page "X" has no linked Instagram Professional account` | Pas de lien Instagram, ou lien créé après `auth` | Liez-le dans Business Suite, puis `rustedin meta pages --account=X --refresh` |
| `<fichier> is not valid JSON` | Configuration corrompue | Corrigez ou déplacez le fichier ; rustedin refuse de l'écraser |

### Authentification

| Message | Cause | Solution |
|---------|-------|----------|
| `Failed to bind port 8765` | Port déjà utilisé | `--port=9000` (déclarez l'URI d'abord), ou libérez le port |
| `Timeout waiting for the OAuth callback (5 min)` | Aucune réponse du navigateur | Relancez et ouvrez l'URL manuellement |
| `Auth failed: state mismatch` | Callback obsolète, ou second flux en cours | Recommencez l'authentification |
| `Auth failed: unauthorized_scope_error` | Produit manquant sur l'app LinkedIn. **Perso :** *Share on LinkedIn* + *Sign In with OpenID Connect*. **Org :** *Community Management API*, et rien d'autre | Ajoutez le produit, puis relancez `auth` |
| `No Facebook Page returned`, tous les scopes accordés | Login for Business n'a délégué aucune ressource — il ne peut pas accorder le portefeuille propriétaire de l'app | Utilisez un token d'utilisateur système : [`meta token`](#meta-token) |
| `Invalid Scopes: <nom>` | Permission indisponible pour l'app, ou retirée par Meta | Retirez-la de `--scopes`, ou ajoutez le cas d'usage qui la porte |
| `The refresh token for "X" expired on …` | Refresh token LinkedIn vieux de plus d'un an | `rustedin linkedin auth --account=X` |

### Publication

| Message | Cause | Solution |
|---------|-------|----------|
| `LinkedIn API 401 …` | Token invalide ou révoqué | `rustedin linkedin auth --account=X` |
| `LinkedIn API 403 …` | Scope manquant, ou le membre qui a autorisé n'est plus admin de la page | Ré-authentifiez avec la bonne app et un compte admin |
| `Commentary is N characters — LinkedIn allows at most 3000` | Texte trop long | Raccourcissez-le |
| `code 190` — Invalid OAuth access token | Token Meta expiré ou révoqué | `rustedin meta auth --account=X` |
| `code 200` / `code 10` — erreur de permission | Scope manquant, ou pas encore validé par l'App Review | `rustedin meta status --check` ; soumettez l'app à la review pour passer en Live |
| `code 100` — paramètre invalide | Le plus souvent une URL de média inaccessible | Rendez l'URL publique, en HTTPS, sans redirection |
| `code 9004` — échec de récupération du média | Meta n'a pas pu télécharger le média | Idem ci-dessus |
| `code 25` | Compte non professionnel, ou non lié à la Page | Convertissez le compte, puis `meta pages --refresh` |
| `Instagram failed to process container …` | Média refusé (format, ratio, durée) | Réencodez ; Instagram veut du JPEG et du MP4 |
| `Timed out … waiting for container` | Traitement plus long que la fenêtre d'attente | `rustedin instagram publish --creation-id=<id>` |
| `Scheduled time … too close` | Moins de 10 minutes à l'avance | Repoussez le timestamp |
| `Instagram cannot publish without media` | `broadcast` avec une cible Instagram et sans `--image` | Ajoutez `--image`, ou retirez la cible |
| `N of M targets failed` | Échec partiel d'un `broadcast` | Lisez l'`error` de chaque cible dans le JSON sur stdout |

Chaque erreur de la Graph API est affichée avec son code, son sous-code, son
`fbtrace_id` et un indice nommant la correction ; les erreurs LinkedIn portent
leur `serviceErrorCode` et leur propre indice.

---

## Codes de sortie

| Code | Signification |
|------|---------------|
| `0` | Succès — un document JSON a été affiché sur stdout |
| `1` | Toute erreur, y compris un échec partiel de `broadcast` — le message est sur stderr |

---

## Contribuer

Les contributions sont les bienvenues ! Lisez [CONTRIBUTING.md](CONTRIBUTING.md)
pour la structure du projet, le modèle de branches (`main` / `develop`,
`feature/*` et `fix/*` → PR vers `develop`), les conventions de commit et le
processus de release. En participant, vous acceptez le
[Code de conduite](CODE_OF_CONDUCT.md).

---

## Sécurité

Vous avez trouvé une vulnérabilité ? **N'ouvrez pas d'issue publique** — voir
[SECURITY.md](SECURITY.md) pour la signaler en privé.

`rustedin.json` contient des tokens OAuth, des tokens de Page et des client
secrets. Il est ignoré par git et écrit en `0600` ; ne le committez ni ne le
partagez jamais.

---

## Licence

MIT © [Quentin Mathis](https://github.com/z29k) — voir [LICENSE](LICENSE).
