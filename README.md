# NOVARIS CAD

Logiciel de CAO 3D paramétrique en Rust, inspiré du flux de travail de SolidWorks :
esquisse contrainte → fonctions → pièce → assemblage → mise en plan → export.

## État : Jalon 4 (mises en plan)

Ce qui fonctionne aujourd'hui :

- **Esquisses** sur les plans de face, de dessus et de droite, sur un plan de référence ou sur une face plane de la pièce.
  - Outils : ligne (en chaîne), rectangle, cercle, arc par le centre, axe (ligne de construction), point, cotation intelligente.
  - Relations : horizontale, verticale, coïncidente, parallèle, perpendiculaire, tangente, égale, milieu, fixe.
  - Les relations horizontale et verticale sont ajoutées automatiquement au dessin, comme dans SolidWorks.
  - Solveur numérique (Levenberg-Marquardt) avec analyse des degrés de liberté. Code couleur SolidWorks : bleu = sous-contraint, noir = contraint, rouge = conflit.
- **Fonctions** (noms à la SolidWorks : Esquisse1, Bossage-Extrusion1, Congé1…) :
  - bossage et enlèvement de matière extrudés : borgne, à travers tout, plan milieu, sens inversé ;
  - bossage et enlèvement avec révolution, autour d'un axe d'esquisse ;
  - perçage simple ou lamé, borgne ou débouchant, à chaque point d'une esquisse ;
  - congé et chanfrein sur les arêtes sélectionnées dans la vue (Maj+clic pour en ajouter) ;
  - répétitions linéaire et circulaire, symétrie ;
  - plans de référence décalés.
- **Arbre de création** :
  - éditer une fonction ou une esquisse, puis tout se reconstruit ;
  - supprimer temporairement (suppress), renommer, effacer (refusé tant que d'autres fonctions en dépendent) ;
  - barre de reconstruction ;
  - une erreur dans une fonction ne bloque pas les autres ;
  - reconstruction incrémentale.
- **Noyau B-Rep exact** : [OpenCascade](https://dev.opencascade.org/) 7.8, le noyau de FreeCAD, via la crate `nova-occt`. Ses booléens sont exacts, y compris pour les faces coplanaires, et le volume aussi. Toute exception OCCT devient une erreur affichée sur la fonction concernée, jamais un plantage.
- **Assemblages** (`.nvasm`) :
  - insertion de pièces `.nvcad` ; la première est fixe, chaque instance est nommée comme dans SolidWorks (« Axe<2> ») ;
  - contraintes standard entre faces planes ou cylindriques : coïncidente, parallèle, perpendiculaire, distance, angle, coaxiale, avec alignement aligné / anti-aligné proposé d'après la position actuelle ;
  - solveur 3D (Levenberg-Marquardt) avec degrés de liberté par composant : « (f) » fixe, « (-) » sous-contraint ; une contrainte contradictoire est refusée sans rien déplacer ;
  - déplacement d'un composant libre à la souris (glisser), rotation avec Maj, puis les contraintes sont réappliquées ;
  - détection d'interférences exacte (volumes communs affichés en rouge), nomenclature, export STEP de l'ensemble ;
  - « Ouvrir la pièce » depuis l'arbre, puis « Enregistrer et revenir à l'assemblage » : les contraintes suivent les faces modifiées.
- **Mises en plan** (`.nvdrw`) :
  - Fichier > « Créer une mise en plan » depuis la pièce ou l'assemblage enregistré : 3 vues standard plus une vue isométrique, à une échelle normalisée choisie pour tenir sur la feuille (A4, A3, A2) ;
  - projection européenne (premier dièdre, par défaut) ou américaine ; vues du modèle dans n'importe quelle orientation, vues projetées alignées sur leur vue parente ;
  - arêtes cachées en pointillés (lignes cachées calculées par OpenCascade), arêtes tangentes, marques de centre ;
  - **Cotation intelligente** : deux sommets (cote horizontale, verticale ou alignée selon l'endroit où on la pose), une arête (longueur), un cercle (Ø) ou un arc (R) ;
  - associativité : après une modification de la pièce, « Mettre à jour » (ou « Ouvrir le modèle » puis « Enregistrer et revenir à la mise en plan ») recalcule les vues ; les cotes suivent les sommets, et une cote dont la géométrie a disparu passe en rouge ;
  - cartouche (titre, fichier, échelle, format, date, matière, symbole de projection) et nomenclature pour un assemblage ;
  - export **PDF** vectoriel et **DXF** (R12, calques VISIBLE, CACHE, AXES, COTATION…).
- **Fichiers** : `.nvcad` (historique paramétrique en JSON, reconstruit à l'ouverture), `.nvdrw` (feuille, vues et cotes ; le modèle est relu à l'ouverture), `.nvasm` (composants, placements et contraintes ; chemins des pièces relatifs au fichier), export STEP (AP214, surfaces exactes) et STL.
- **Interface** egui + wgpu :
  - CommandManager, FeatureManager, PropertyManager ;
  - vue 3D orthographique, sélection de faces et d'arêtes ;
  - annuler et rétablir.

Limites connues :

- **Nommage des arêtes** : un congé repère ses arêtes par leur position (le point milieu). Si une modification déplace ou supprime fortement une arête, la fonction passe en erreur et il faut la ressélectionner.
- **Calculs synchrones** : l'interface se fige pendant les calculs, ce qui reste bref sur des pièces simples.
- **Contraintes d'assemblage** : les faces sont repérées par leur position, comme les arêtes des congés. Pas encore de sous-assemblages, de contraintes mécaniques (engrenage, came, tangente) ni de vues éclatées ; l'export STL d'un assemblage n'est pas disponible.
- **Mises en plan** : une seule feuille ; pas encore de vues en coupe ou de détail, de bulles, de tolérances ni d'import des cotes d'esquisse ; les cotes mesurent le modèle mais ne le pilotent pas.
- **Pas encore disponibles** : balayages et lissages, tôlerie, configurations.
- **Fichiers SolidWorks** : les `.sldprt` (format propriétaire) ne sont pas lisibles ; l'échange se fait en STEP.
- **Dialogues de fichiers** : on saisit le chemin à la main.

## Compiler et lancer (Linux)

Prérequis : Rust stable, `cmake` et un compilateur C++, plus Vulkan ou EGL (Mesa) pour le rendu.

```bash
# Arch / CachyOS
sudo pacman -S cmake base-devel
# Debian / Ubuntu
sudo apt install cmake build-essential

cargo run --release -p novaris-desktop            # nouvelle pièce
cargo run --release -p novaris-desktop -- piece.nvcad
cargo run --release -p novaris-desktop --example demo_part -- plaque.nvcad   # pièce d'exemple
cargo run --release -p novaris-desktop --example demo_assembly -- demo_assemblage
cargo run --release -p novaris-desktop -- demo_assemblage/ensemble.nvasm      # assemblage d'exemple
cargo run --release -p novaris-desktop --example demo_drawing -- demo_plan demo_assemblage/ensemble.nvasm
cargo run --release -p novaris-desktop -- demo_plan/plaque.nvdrw              # mise en plan d'exemple
cargo test --workspace
```

La première compilation construit OpenCascade depuis ses sources : environ 35 min sur 4 cœurs. Le résultat est ensuite mis en cache dans `target/OCCT` et partagé entre les profils debug et release. Pour réutiliser une installation d'OCCT 7.8 existante, pointez `NOVA_OCCT_DIR` vers le dossier qui contient `include/` et `lib/`.

Windows et macOS devraient fonctionner avec la même chaîne (cmake + compilateur C++), mais ils ne sont pas encore testés.

## Prise en main

1. Cliquez sur **Plan de dessus** dans l'arbre (ou sur une face de la pièce), puis sur **Esquisse**.
2. Dessinez avec **Rectangle**, **Cercle**… puis cotez avec **Cotation intelligente**. Tapez la valeur puis Entrée.
3. Cliquez sur **Quitter l'esquisse**, puis sur **Bossage/Base extrudé** et réglez la profondeur dans le PropertyManager.
4. Pour un congé : cliquez sur une arête de la pièce (Maj+clic pour en ajouter), puis sur **Congé**.
5. Double-cliquez sur une fonction de l'arbre pour la modifier.

Mise en plan : enregistrez la pièce, puis **Fichier > Créer une mise en plan**. Cliquez **Cotation intelligente**, puis deux sommets (ou une arête, ou un cercle), et cliquez pour poser la cote. Glissez une vue pour la déplacer ; Échap revient à la sélection. **Fichier > Exporter PDF / DXF** produit les fichiers pour l'atelier.

Assemblage : **Fichier > Nouvel assemblage**, insérez une pièce, puis d'autres. Cliquez **Contrainte**, cliquez deux faces dans la vue, choisissez le type et validez avec ✔ ; le PropertyManager reste ouvert pour la contrainte suivante, comme dans SolidWorks.

| Souris / clavier | Action |
|---|---|
| Bouton du milieu (ou droit) glissé | Rotation |
| Bouton gauche glissé (assemblage) | Déplacer un composant libre (Maj : le faire pivoter) |
| Ctrl + bouton du milieu glissé | Translation |
| Molette | Zoom sur le curseur |
| F | Zoom au mieux |
| Ctrl+1 / 4 / 5 / 7 / 8 | Vue de face / droite / dessus / isométrique / normale à |
| Ctrl+Z / Ctrl+Y | Annuler / rétablir |
| Ctrl+S / Ctrl+O / Ctrl+N | Enregistrer / ouvrir / nouveau |
| Suppr / Échap | Effacer la sélection / annuler l'outil |

## Architecture

```
crates/
  nova-core       types communs, identifiants, erreurs
  nova-occt       liaisons minimales et sûres vers OpenCascade (pont cxx + couche C++)
  nova-kernel     plans, profils, outils (prisme, révolution, transformations), solides
  nova-sketch     esquisses 2D, solveur de contraintes, extraction de contours
  nova-features   arbre de création paramétrique et reconstruction
  nova-io         fichiers .nvcad, export STEP / STL
  nova-assembly   assemblages : composants, contraintes, solveur 3D, interférences, nomenclature
  nova-drawing    mises en plan : vues projetées, cotes associatives, cartouche, PDF et DXF
  nova-ui         application egui : caméra, rendu wgpu, édition d'esquisse, modes assemblage et mise en plan
  (nova-sheet-metal, nova-interaction, nova-shaders, nova-commands :
   prototypes antérieurs, pas encore reliés à l'application)
apps/desktop      exécutable `novaris`
```

## Feuille de route

- **J5** : balayages et lissages, coques, nervures.
- **J6** : tôlerie, configurations, équations.
- **J7** : vues en coupe et de détail, bulles, tolérances dans les mises en plan.

Licences : NOVARIS CAD est sous AGPL-3.0-or-later. OpenCascade est sous LGPL-2.1, avec l'exception d'OCCT.
