# NOVARIS CAD

Logiciel de CAO 3D paramétrique en Rust, inspiré du flux de travail de SolidWorks :
esquisse contrainte → fonctions → pièce → export.

## État : Jalon 1 (pièce de bout en bout)

Ce qui fonctionne aujourd'hui :

- **Esquisses** sur les plans de face, de dessus et de droite, ou sur une face plane de la pièce.
  - Outils : ligne (en chaîne), rectangle, cercle, arc par le centre, cotation intelligente.
  - Relations : horizontale, verticale, coïncidente, parallèle, perpendiculaire, tangente, égale, milieu, fixe.
  - Les relations horizontale et verticale sont ajoutées automatiquement au dessin, comme dans SolidWorks.
  - Solveur numérique (Levenberg-Marquardt) avec analyse des degrés de liberté. Code couleur SolidWorks : bleu = sous-contraint, noir = contraint, rouge = conflit.
- **Fonctions** : bossage et enlèvement de matière extrudés (borgne, à travers tout, plan milieu, sens inversé).
  - Les noms suivent SolidWorks en français : Esquisse1, Bossage-Extrusion1…
- **Arbre de création** :
  - éditer une fonction ou une esquisse, puis tout se reconstruit ;
  - supprimer temporairement (suppress), renommer, effacer ;
  - barre de reconstruction ;
  - une erreur dans une fonction ne bloque pas les autres ;
  - reconstruction incrémentale (seules les fonctions modifiées et leurs suivantes sont recalculées).
- **Noyau B-Rep exact** basé sur [Truck](https://github.com/ricosjp/truck) (pur Rust) : extrusions, booléens, tessellation.
- **Fichiers** : `.nvcad` (historique paramétrique en JSON, reconstruit à l'ouverture), export STEP et STL.
- **Interface** egui + wgpu :
  - CommandManager, FeatureManager, PropertyManager ;
  - vue 3D orthographique avec arêtes et sélection de faces ;
  - annuler et rétablir.

Limites connues :

- **Faces coplanaires** : les booléens de Truck échouent quand des faces se touchent exactement dans le même plan, ce qui est le cas normal en CAO (un bossage posé sur une face). Le noyau réessaie alors avec l'outil agrandi d'environ 1 µm. L'erreur de volume reste inférieure à 1e-4 en relatif.
- **Perçages lents** : un perçage cylindrique coûte environ 0,5 à 1 s (coût propre à Truck). La reconstruction est synchrone, donc l'interface se fige pendant le calcul.
- **Pas encore disponibles** : révolution, congés, chanfreins, répétitions, assemblages, mises en plan.
- **Fichiers SolidWorks** : les `.sldprt` (format propriétaire) ne sont pas lisibles ; l'échange se fait en STEP.
- **Dialogues de fichiers** : on saisit le chemin à la main.

## Compiler et lancer

```bash
cargo run --release -p novaris-desktop            # nouvelle pièce
cargo run --release -p novaris-desktop -- piece.nvcad
cargo run --release -p novaris-desktop --example demo_part -- plaque.nvcad   # pièce d'exemple
cargo test --workspace
```

Sous Linux, wgpu a besoin de Vulkan ou d'EGL (pilotes Mesa).

## Prise en main

1. Cliquez sur **Plan de dessus** dans l'arbre (ou sur une face de la pièce), puis sur **Esquisse**.
2. Dessinez avec **Rectangle**, **Cercle**… puis cotez avec **Cotation intelligente**. Tapez la valeur puis Entrée.
3. Cliquez sur **Quitter l'esquisse**, puis sur **Bossage/Base extrudé** et réglez la profondeur dans le PropertyManager.
4. Double-cliquez sur une fonction de l'arbre pour la modifier.

| Souris / clavier | Action |
|---|---|
| Bouton du milieu (ou droit) glissé | Rotation |
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
  nova-kernel     noyau B-Rep (Truck) : plans, profils, solides, booléens, STEP
  nova-sketch     esquisses 2D, solveur de contraintes, extraction de contours
  nova-features   arbre de création paramétrique et reconstruction
  nova-io         fichiers .nvcad, export STEP / STL
  nova-ui         application egui : caméra, rendu wgpu, édition d'esquisse
  (nova-assembly, nova-sheet-metal, nova-interaction, nova-shaders, nova-commands :
   prototypes antérieurs, pas encore reliés à l'application)
apps/desktop      exécutable `novaris`
```

## Feuille de route

- **J2** : révolution, congé, chanfrein, assistant de perçage, répétitions, symétrie, plans de référence.
- **J3** : assemblages avec contraintes.
- **J4** : mises en plan (projections, cotes, PDF/DXF).
- **J5** : tôlerie, configurations, équations.

Licence : AGPL-3.0-or-later.
