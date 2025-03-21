import {Button, CircularProgress} from "@mui/material";

export const BaseLoadingButton = function(props) {

  if(props.loading) {

    return <Button variant={props.variant || "outlined" } disabled={true} color={props.color} endIcon={<CircularProgress  size="20px" />}>{props.children}</Button>;
  } else {
    return <Button onClick={props.onClick} variant={props.variant || "outlined" } color={props.color} endIcon={props.endIcon}>{props.children}</Button>;
  }
}

export const PrimaryLoadingButton = function(props) {

  return (<BaseLoadingButton onClick={props.onClick}
                         variant={props.variant}
                         endIcon={props.endIcon}
                         loading={props.working || props.loading }
                         >{props.children}</BaseLoadingButton>);
}
export const SecondaryLoadingButton = function(props) {
  return (<BaseLoadingButton variant={props.variant}
                         onClick={props.onClick}
                         endIcon={props.endIcon}
                         loading={props.working || props.loading }
                         color={"secondary"}
  >{props.children}</BaseLoadingButton>);
}
export const DangerLoadingButton = function(props) {
  return (<BaseLoadingButton onClick={props.onClick}
                            variant={props.variant}
                            endIcon={props.endIcon}
                            loading={props.working || props.loading }
                            color={"error"}
  >{props.children}</BaseLoadingButton>);
}