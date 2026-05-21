{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    let
      craneLib = inputs.crane.mkLib pkgs;
    in
    {
      devShells.default = craneLib.devShell {
        packages = with pkgs; [ watchexec ];
      };
    };
}
